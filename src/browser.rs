use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Serialize, Deserialize)]
struct ServerState {
    token: String,
    files: Vec<PathBuf>,
    timeout_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadyState {
    ok: bool,
    url: Option<String>,
    expires_at_unix: Option<u64>,
    error: Option<String>,
}

struct SharedFile {
    name: String,
    file: File,
    length: usize,
}

#[derive(Debug)]
pub struct BrowserLaunch {
    pub url: String,
    pub expires_at_unix: u64,
    pub transfer_id: String,
}

pub fn availability() -> std::result::Result<IpAddr, String> {
    let runtime = runtime_directory().map_err(|error| error.to_string())?;
    if !runtime.is_dir() {
        return Err("The per-user runtime directory is unavailable.".into());
    }
    if !command_exists("systemd-run") || !command_exists("systemctl") {
        return Err("The user service manager tools are unavailable.".into());
    }
    lan_address().map_err(|error| error.to_string())
}

pub fn launch(
    paths: &[PathBuf],
    timeout_seconds: u64,
    dry_run: bool,
) -> Result<Option<BrowserLaunch>> {
    validate_browser_files(paths)?;
    availability().map_err(|detail| anyhow!(detail))?;
    if dry_run {
        return Ok(None);
    }

    let runtime = private_runtime_directory()?;
    let transfer_id = random_hex(16)?;
    let token = random_hex(32)?;
    let state_path = runtime.join(format!("{transfer_id}.state.json"));
    let ready_path = runtime.join(format!("{transfer_id}.ready.json"));
    let state = ServerState {
        token,
        files: paths.to_vec(),
        timeout_seconds,
    };
    write_private_json(&state_path, &state)?;

    let executable = env::current_exe().context("could not locate the unified-share executable")?;
    let unit = unit_name(&transfer_id)?;
    let mut child = Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect", "--unit"])
        .arg(&unit)
        .arg(executable)
        .args(["browser-serve", "--state"])
        .arg(&state_path)
        .arg("--ready")
        .arg(&ready_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not start the Browser / QR server")?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut launcher_finished = false;
    loop {
        if ready_path.exists() {
            let ready: ReadyState = read_json(&ready_path)?;
            let _ = fs::remove_file(&ready_path);
            if !ready.ok {
                bail!(
                    "Browser / QR server failed: {}",
                    ready
                        .error
                        .unwrap_or_else(|| "unknown startup error".into())
                );
            }
            return Ok(Some(BrowserLaunch {
                url: ready.url.context("Browser / QR server omitted its URL")?,
                expires_at_unix: ready
                    .expires_at_unix
                    .context("Browser / QR server omitted its expiry")?,
                transfer_id,
            }));
        }
        if !launcher_finished
            && let Some(status) = child
                .try_wait()
                .context("could not monitor Browser / QR launcher")?
        {
            if !status.success() {
                let _ = fs::remove_file(&state_path);
                bail!("Browser / QR launcher exited during startup with {status}");
            }
            launcher_finished = true;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = Command::new("systemctl")
                .args(["--user", "stop", &unit])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = fs::remove_file(&state_path);
            bail!("Browser / QR server did not become ready within five seconds");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn stop(transfer_id: &str) -> Result<()> {
    let unit = unit_name(transfer_id)?;
    let status = Command::new("systemctl")
        .args(["--user", "stop", &unit])
        .status()
        .context("could not ask the user service manager to stop the transfer")?;
    if !status.success() {
        bail!("could not stop Browser / QR transfer {transfer_id}");
    }
    let runtime = private_runtime_directory()?;
    let _ = fs::remove_file(runtime.join(format!("{transfer_id}.state.json")));
    let _ = fs::remove_file(runtime.join(format!("{transfer_id}.ready.json")));
    Ok(())
}

pub fn serve_from_state(state_path: &Path, ready_path: &Path) -> Result<()> {
    let result = serve_inner(state_path, ready_path);
    if let Err(error) = &result {
        let ready = ReadyState {
            ok: false,
            url: None,
            expires_at_unix: None,
            error: Some(format!("{error:#}")),
        };
        let _ = write_private_json(ready_path, &ready);
    }
    let _ = fs::remove_file(state_path);
    result
}

fn serve_inner(state_path: &Path, ready_path: &Path) -> Result<()> {
    ensure_runtime_file(state_path)?;
    ensure_runtime_file(ready_path.parent().context("invalid ready path")?)?;
    let state: ServerState = read_json(state_path)?;
    fs::remove_file(state_path).context("could not remove consumed Browser / QR state")?;
    validate_browser_files(&state.files)?;
    let files = open_shared_files(&state.files)?;
    if state.timeout_seconds < 30 || state.timeout_seconds > 86_400 {
        bail!("invalid Browser / QR timeout");
    }
    if state.token.len() != 64 || !state.token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid Browser / QR token");
    }

    let address = lan_address()?;
    let listener = TcpListener::bind(SocketAddr::new(address, 0))
        .context("could not bind the Browser / QR LAN server")?;
    let port = listener.local_addr()?.port();
    let server = Server::from_listener(listener, None)
        .map_err(|error| anyhow!("could not create Browser / QR server: {error}"))?;
    let base = format!("http://{address}:{port}/t/{}/", state.token);
    let expires_at_unix = unix_now()?.saturating_add(state.timeout_seconds);
    write_private_json(
        ready_path,
        &ReadyState {
            ok: true,
            url: Some(base.clone()),
            expires_at_unix: Some(expires_at_unix),
            error: None,
        },
    )?;

    let deadline = Instant::now() + Duration::from_secs(state.timeout_seconds);
    while Instant::now() < deadline {
        let wait = (deadline - Instant::now()).min(Duration::from_secs(1));
        if let Some(request) = server.recv_timeout(wait)? {
            handle_request(request, &state.token, &files, &base);
        }
    }
    Ok(())
}

fn handle_request(request: tiny_http::Request, token: &str, files: &[SharedFile], base: &str) {
    let index_prefix = format!("/t/{token}/file/");
    let index_path = format!("/t/{token}/");
    let response = if request.method() != &Method::Get && request.method() != &Method::Head {
        empty_response(StatusCode(405))
    } else if request.url() == index_path {
        html_response(index_html(files, base))
    } else if let Some(raw_index) = request.url().strip_prefix(&index_prefix) {
        match raw_index
            .parse::<usize>()
            .ok()
            .and_then(|index| files.get(index))
        {
            Some(file) => match file_response(file, request.method() == &Method::Head) {
                Ok(response) => response,
                Err(_) => empty_response(StatusCode(500)),
            },
            None => empty_response(StatusCode(404)),
        }
    } else {
        empty_response(StatusCode(404))
    };
    let _ = request.respond(response);
}

fn file_response(shared: &SharedFile, head: bool) -> Result<Response<Box<dyn Read + Send>>> {
    let disposition = format!(
        "attachment; filename=\"{}\"",
        safe_header_filename(&shared.name)
    );
    let mut response: Response<Box<dyn Read + Send>> = if head {
        Response::new(
            StatusCode(200),
            Vec::new(),
            Box::new(std::io::empty()),
            Some(shared.length),
            None,
        )
    } else {
        Response::new(
            StatusCode(200),
            Vec::new(),
            Box::new(shared.file.try_clone()?),
            Some(shared.length),
            None,
        )
    };
    response.add_header(Header::from_bytes("Content-Type", "application/octet-stream").unwrap());
    response.add_header(Header::from_bytes("Content-Disposition", disposition).unwrap());
    response.add_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    Ok(response)
}

fn empty_response(status: StatusCode) -> Response<Box<dyn Read + Send>> {
    Response::new(
        status,
        Vec::new(),
        Box::new(std::io::empty()),
        Some(0),
        None,
    )
}

fn html_response(body: String) -> Response<Box<dyn Read + Send>> {
    let bytes = body.into_bytes();
    let length = bytes.len();
    let mut response: Response<Box<dyn Read + Send>> = Response::new(
        StatusCode(200),
        Vec::new(),
        Box::new(std::io::Cursor::new(bytes)),
        Some(length),
        None,
    );
    response.add_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
    response.add_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    response.add_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    response.add_header(Header::from_bytes("X-Frame-Options", "DENY").unwrap());
    response.add_header(Header::from_bytes("Referrer-Policy", "no-referrer").unwrap());
    response.add_header(
        Header::from_bytes(
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'",
        )
        .unwrap(),
    );
    response
}

fn index_html(files: &[SharedFile], base: &str) -> String {
    let mut links = String::new();
    for (index, file) in files.iter().enumerate() {
        links.push_str(&format!(
            "<li><a href=\"{}file/{}\" download>{}</a></li>",
            base,
            index,
            html_escape(&file.name)
        ));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Unified Share</title><style>body{{font:16px system-ui;max-width:38rem;margin:3rem auto;padding:0 1rem;color:#18181b}}h1{{font-size:1.5rem}}li{{margin:.8rem 0}}a{{color:#2563eb}}</style></head><body><h1>Shared files</h1><p>This private link expires automatically.</p><ul>{links}</ul></body></html>"
    )
}

fn open_shared_files(paths: &[PathBuf]) -> Result<Vec<SharedFile>> {
    paths
        .iter()
        .map(|path| {
            let file = File::open(path)
                .with_context(|| format!("could not pin shared file {}", path.display()))?;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                bail!(
                    "shared path changed before it could be pinned: {}",
                    path.display()
                );
            }
            Ok(SharedFile {
                name: path
                    .file_name()
                    .context("shared file has no name")?
                    .to_string_lossy()
                    .into_owned(),
                file,
                length: usize::try_from(metadata.len())
                    .context("shared file is too large for this platform")?,
            })
        })
        .collect()
}

fn validate_browser_files(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        bail!("Browser / QR sharing requires at least one file");
    }
    for path in paths {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("could not inspect {}", path.display()))?;
        if !metadata.file_type().is_file() {
            bail!(
                "Browser / QR currently accepts regular files only: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn runtime_directory() -> Result<PathBuf> {
    let path = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(path))
}

fn private_runtime_directory() -> Result<PathBuf> {
    let directory = runtime_directory()?.join("unified-share");
    fs::create_dir_all(&directory)
        .context("could not create the Unified Share runtime directory")?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}

fn ensure_runtime_file(path: &Path) -> Result<()> {
    let expected = private_runtime_directory()?.canonicalize()?;
    let candidate = if path.is_dir() {
        path.canonicalize()?
    } else {
        path.parent()
            .context("runtime path has no parent")?
            .canonicalize()?
    };
    if candidate != expected {
        bail!("Browser / QR state must be inside the private runtime directory");
    }
    Ok(())
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("could not decode {}", path.display()))
}

fn lan_address() -> Result<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 80))?;
    let address = socket.local_addr()?.ip();
    if address.is_loopback() || address.is_unspecified() {
        bail!("No LAN address is currently available.");
    }
    Ok(address)
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value)
        .map_err(|error| anyhow!("could not obtain secure randomness: {error}"))?;
    Ok(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn unit_name(transfer_id: &str) -> Result<String> {
    if transfer_id.len() != 32
        || !transfer_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("invalid Browser / QR transfer ID");
    }
    Ok(format!("unified-share-browser-{transfer_id}"))
}

fn command_exists(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|directory| directory.join(name).is_file()))
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn safe_header_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '"' | '\\' | '\r' | '\n' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_256_bits_and_hex_encoded() {
        let token = random_hex(32).unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn html_escapes_untrusted_file_names() {
        assert_eq!(html_escape("<x & \"y\">"), "&lt;x &amp; &quot;y&quot;&gt;");
    }

    #[test]
    fn header_file_names_cannot_inject_headers() {
        assert_eq!(
            safe_header_filename("x\r\nInjected: yes"),
            "x__Injected: yes"
        );
    }

    #[test]
    fn index_contains_only_numeric_allowlist_routes() {
        let first = File::open("README.md").unwrap();
        let second = first.try_clone().unwrap();
        let html = index_html(
            &[
                SharedFile {
                    name: "a.txt".into(),
                    file: first,
                    length: 1,
                },
                SharedFile {
                    name: "b.txt".into(),
                    file: second,
                    length: 1,
                },
            ],
            "http://host/t/token/",
        );
        assert!(html.contains("http://host/t/token/file/0"));
        assert!(html.contains("http://host/t/token/file/1"));
        assert!(!html.contains("/tmp/"));
    }

    #[test]
    fn transfer_ids_cannot_escape_the_unit_namespace() {
        assert!(unit_name("0123456789abcdef0123456789abcdef").is_ok());
        assert!(unit_name("../../another.service").is_err());
        assert!(unit_name("ABCDEF0123456789abcdef0123456789").is_err());
    }
}
