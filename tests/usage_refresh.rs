use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

const NOW: i64 = 1_789_700_000_000;
const INITIAL_NEXT_FETCH_AT: i64 = NOW + 15_000;
const FIXTURE: &str = include_str!("fixtures/usage-api-2026-09-18.json");

#[derive(Clone, Debug)]
struct Recorded {
    request_line: String,
    headers: Vec<(String, String)>,
    cache_bytes_at_request: Option<Vec<u8>>,
}

#[derive(Clone)]
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

struct FakeServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    response: Arc<Mutex<Response>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeServer {
    fn new(cache_path: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake HTTP server");
        let addr = listener.local_addr().expect("fake HTTP server address");
        listener
            .set_nonblocking(true)
            .expect("set fake HTTP server nonblocking");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let response = Arc::new(Mutex::new(Response {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            delay: Duration::ZERO,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let requests_for_thread = Arc::clone(&requests);
        let response_for_thread = Arc::clone(&response);
        let stop_for_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        handle_connection(
                        stream,
                        &cache_path,
                        &requests_for_thread,
                        &response_for_thread,
                    )},
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            addr,
            requests,
            response,
            stop,
            thread: Some(thread),
        }
    }

    fn closed(_cache_path: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed fake server");
        let addr = listener.local_addr().expect("closed fake server address");
        drop(listener);
        Self {
            addr,
            requests: Arc::new(Mutex::new(Vec::new())),
            response: Arc::new(Mutex::new(Response {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
                delay: Duration::ZERO,
            })),
            stop: Arc::new(AtomicBool::new(true)),
            thread: None,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("lock requests").clone()
    }

    fn set_response(
        &self,
        status: u16,
        headers: Vec<(String, String)>,
        body: impl Into<Vec<u8>>,
        delay: Duration,
    ) {
        *self.response.lock().expect("lock response") = Response {
            status,
            headers,
            body: body.into(),
            delay,
        };
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join fake HTTP server");
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    cache_path: &Path,
    requests: &Mutex<Vec<Recorded>>,
    response: &Mutex<Response>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set request read timeout");
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => panic!("read fake request: {error}"),
        }
    }

    let request = String::from_utf8_lossy(&raw);
    let mut lines = request.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    requests.lock().expect("lock requests").push(Recorded {
        request_line,
        headers,
        cache_bytes_at_request: fs::read(cache_path).ok(),
    });

    let response = response.lock().expect("lock response").clone();
    thread::sleep(response.delay);
    let reason = match response.status {
        200 => "OK",
        302 => "Found",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        _ => "Test Response",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason,
        response.body.len()
    );
    for (name, value) in response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&response.body));
    let _ = stream.shutdown(Shutdown::Both);
}

struct CaseDir {
    path: PathBuf,
    initial_cache: Vec<u8>,
}

impl CaseDir {
    fn new(case_number: usize) -> Self {
        let path = std::env::temp_dir().join(format!(
            "usage_refresh_s04_{}_{}",
            std::process::id(),
            case_number
        ));
        fs::create_dir(&path).expect("create fresh case directory");
        fs::create_dir(path.join("usage")).expect("create usage directory");
        let initial_cache = format!(
            r#"{{"fetchedAt":{},"nextFetchAt":{},"limits":[{{"displayName":"Fable","percent":60.0,"resetsAt":{},"isActive":true}}]}}"#,
            NOW - 600_000,
            INITIAL_NEXT_FETCH_AT,
            NOW + 1_000_000,
        )
        .into_bytes();
        fs::write(path.join("usage/cache.json"), &initial_cache).expect("write initial cache");
        fs::write(
            path.join("credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":" test-token \n"}}"#,
        )
        .expect("write credentials");
        Self {
            path,
            initial_cache,
        }
    }

    fn cache_path(&self) -> PathBuf {
        self.path.join("usage/cache.json")
    }

    fn lock_path(&self) -> PathBuf {
        self.path.join("usage/refresh.lock")
    }

    fn run(&self, server: &FakeServer) -> Output {
        let path = std::env::var_os("PATH").unwrap_or_default();
        Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
            .arg("usage-refresh")
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.path)
            .env("PPULSE_CONFIG_DIR", &self.path)
            .env("PPULSE_NOW_MS", NOW.to_string())
            .env("PPULSE_USAGE_API_URL", server.url())
            .env(
                "PPULSE_USAGE_TOKEN_FILE",
                self.path.join("credentials.json"),
            )
            .env("COLUMNS", "120")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn usage-refresh")
            .wait_with_output()
            .expect("wait for usage-refresh")
    }

    fn cache_bytes(&self) -> Vec<u8> {
        fs::read(self.cache_path()).expect("read cache")
    }

    fn cache(&self) -> Value {
        serde_json::from_slice(&self.cache_bytes()).expect("parse cache")
    }
}

impl Drop for CaseDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_silent_and_token_free(output: &Output, case: &CaseDir) {
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    assert!(
        !case
            .cache_bytes()
            .windows(b"test-token".len())
            .any(|bytes| bytes == b"test-token")
    );
}

fn assert_preserved(cache: &Value, next_fetch_at: i64) {
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["nextFetchAt"], json!(next_fetch_at));
    assert_eq!(cache["limits"][0]["displayName"], json!("Fable"));
    assert_eq!(cache["limits"][0]["percent"], json!(60.0));
    assert_eq!(cache["limits"][0]["resetsAt"], json!(NOW + 1_000_000));
    assert_eq!(cache["limits"][0]["isActive"], json!(true));
}

fn assert_success(cache: &Value, next_fetch_at: i64) {
    assert_eq!(cache["fetchedAt"], json!(NOW));
    assert_eq!(cache["nextFetchAt"], json!(next_fetch_at));
    assert_eq!(cache["limits"].as_array().map(Vec::len), Some(1));
    assert_eq!(cache["limits"][0]["displayName"], json!("Fable"));
    assert_eq!(cache["limits"][0]["percent"], json!(65.0));
    assert_eq!(cache["limits"][0]["resetsAt"], json!(1_790_226_000_426_i64));
    assert_eq!(cache["limits"][0]["isActive"], json!(true));
}

fn assert_finished(case: &CaseDir, cache: &Value) {
    assert!(!case.lock_path().exists());
    assert_ne!(cache["nextFetchAt"], json!(INITIAL_NEXT_FETCH_AT));
}

fn header<'a>(request: &'a Recorded, wanted: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value.as_str())
}

/// REQ-01 / REQ-02 / REQ-03 / S-04: usage-refresh handles HTTP, locking, and scheduling end to end.
#[test]
fn test_s04_refresh_subcommand() {
    {
        let case = CaseDir::new(1);
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(0));
        let cache = case.cache();
        assert_success(&cache, NOW + 300_000);
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].request_line.starts_with("GET /api/oauth/usage"));
        assert_eq!(
            header(&requests[0], "authorization"),
            Some("Bearer test-token")
        );
        assert_eq!(
            header(&requests[0], "anthropic-beta"),
            Some("oauth-2025-04-20")
        );
        assert!(requests[0].cache_bytes_at_request.is_some());
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(2);
        let server = FakeServer::new(case.cache_path());
        server.set_response(
            429,
            vec![("Retry-After".into(), "120".into())],
            "{}",
            Duration::ZERO,
        );
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(3));
        let cache = case.cache();
        assert_preserved(&cache, NOW + 120_000);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(3);
        let server = FakeServer::new(case.cache_path());
        server.set_response(401, vec![], "{}", Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(2));
        let cache = case.cache();
        assert_preserved(&cache, NOW + 300_000);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(4);
        let server = FakeServer::closed(case.cache_path());
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(4));
        let cache = case.cache();
        assert_preserved(&cache, NOW + 30_000);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(5);
        let server = FakeServer::new(case.cache_path());
        server.set_response(
            302,
            vec![("Location".into(), "http://127.0.0.1:9/never".into())],
            "",
            Duration::ZERO,
        );
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(4));
        let cache = case.cache();
        assert_preserved(&cache, NOW + 30_000);
        assert_eq!(server.requests().len(), 1);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(6);
        let server = FakeServer::new(case.cache_path());
        let prefix = r#"{"limits":[],"pad":""#;
        let suffix = r#""}"#;
        let body = format!(
            "{prefix}{}{suffix}",
            "x".repeat(70_000 - prefix.len() - suffix.len())
        );
        assert_eq!(body.len(), 70_000);
        assert!(serde_json::from_str::<Value>(&body).is_ok());
        server.set_response(200, vec![], body, Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(4));
        let cache = case.cache();
        assert_preserved(&cache, NOW + 30_000);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(7);
        fs::write(case.lock_path(), "other").expect("write fresh lock");
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(5));
        assert_eq!(server.requests().len(), 0);
        assert_eq!(case.cache_bytes(), case.initial_cache);
        assert_eq!(fs::read_to_string(case.lock_path()).unwrap(), "other");
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(8);
        fs::write(case.lock_path(), "other").expect("write stale lock");
        fs::File::open(case.lock_path())
            .expect("open stale lock")
            .set_modified(SystemTime::now() - Duration::from_secs(120))
            .expect("set stale lock mtime");
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(0));
        let cache = case.cache();
        assert_success(&cache, NOW + 300_000);
        assert_eq!(server.requests().len(), 1);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }

    {
        let case = CaseDir::new(9);
        fs::write(
            case.path.join("settings.json"),
            r#"{"usage":{"refreshSec":120}}"#,
        )
        .expect("write settings");
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let output = case.run(&server);
        assert_eq!(output.status.code(), Some(0));
        let cache = case.cache();
        assert_success(&cache, NOW + 120_000);
        assert_finished(&case, &cache);
        assert_silent_and_token_free(&output, &case);
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn render_stdin_json() -> Value {
    json!({
        "session_id": "usage-refresh-s03",
        "cwd": "/tmp/phosphorpulse-usage-refresh-s03",
        "model": {"display_name": "Fable"},
        "context_window": {"used_percentage": 42},
        "rate_limits": {}
    })
}

fn poll_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    condition()
}

struct RenderDir {
    path: PathBuf,
}

impl RenderDir {
    fn new(case_number: usize) -> Self {
        let path = std::env::temp_dir().join(format!(
            "usage_refresh_s03_{}_{}",
            std::process::id(),
            case_number
        ));
        fs::create_dir(&path).expect("create fresh render case directory");
        fs::write(
            path.join("credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":" test-token \n"}}"#,
        )
        .expect("write credentials");
        Self { path }
    }

    fn write_settings(&self, with_limit_model: bool) {
        let segments = if with_limit_model {
            r#"["limitModel"]"#
        } else {
            r#"["model"]"#
        };
        fs::write(
            self.path.join("settings.json"),
            format!(r#"{{"rows":[{{"layout":"auto","segments":{segments}}}]}}"#),
        )
        .expect("write settings");
    }

    fn write_cache(&self, contents: impl AsRef<[u8]>) {
        let usage_dir = self.path.join("usage");
        fs::create_dir_all(&usage_dir).expect("create usage directory");
        fs::write(usage_dir.join("cache.json"), contents).expect("write cache fixture");
    }

    fn usage_dir_exists(&self) -> bool {
        self.path.join("usage").exists()
    }

    fn cache_path(&self) -> PathBuf {
        self.path.join("usage/cache.json")
    }

    fn cache_bytes(&self) -> Vec<u8> {
        fs::read(self.cache_path()).expect("read cache")
    }

    fn cache(&self) -> Value {
        serde_json::from_slice(&self.cache_bytes()).expect("parse cache")
    }

    fn token_file(&self) -> PathBuf {
        self.path.join("credentials.json")
    }
}

impl Drop for RenderDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_render(
    dir: &Path,
    server_url: &str,
    token_file: &Path,
    stdin_json: &Value,
) -> (Output, Duration) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("render")
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("PPULSE_CONFIG_DIR", dir)
        .env("PPULSE_NOW_MS", NOW.to_string())
        .env("PPULSE_USAGE_API_URL", server_url)
        .env("PPULSE_USAGE_TOKEN_FILE", token_file)
        .env("COLUMNS", "120")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn render");
    child
        .stdin
        .take()
        .expect("render stdin")
        .write_all(
            serde_json::to_string(stdin_json)
                .expect("serialize render stdin")
                .as_bytes(),
        )
        .expect("write render stdin");
    let start = Instant::now();
    let output = child.wait_with_output().expect("wait for render");
    let elapsed = start.elapsed();
    (output, elapsed)
}

/// REQ-03 / S-03: render claims a stale/missing cache, spawns `usage-refresh`
/// (stdio null, no wait), and cold-starts on a fresh claim by polling the
/// cache file for up to 800ms.
#[test]
fn test_s03_trigger_claim_and_cold_start() {
    let stdin_json = render_stdin_json();

    // macOS pays a 200-400ms first-exec disk-scan tax on a freshly built
    // binary; warm up once (rows without limitModel) before any timed case.
    {
        let warm = RenderDir::new(0);
        warm.write_settings(false);
        let server = FakeServer::new(warm.cache_path());
        let (output, _) = run_render(&warm.path, &server.url(), &warm.token_file(), &stdin_json);
        assert!(output.status.success(), "warm-up render failed: {output:?}");
    }

    // (1) fresh cache: render must not claim or spawn a refresh.
    {
        let case = RenderDir::new(1);
        case.write_settings(true);
        case.write_cache(
            serde_json::to_vec(&json!({
                "fetchedAt": NOW - 10_000,
                "nextFetchAt": NOW + 290_000,
                "limits": [{"displayName": "Fable", "percent": 60, "resetsAt": null, "isActive": true}]
            }))
            .unwrap(),
        );
        let server = FakeServer::new(case.cache_path());
        let (output, _elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("Fable"), "S-03 case 1 text: {text}");
        assert!(text.contains("60%"), "S-03 case 1 text: {text}");
        assert_eq!(server.requests().len(), 0, "S-03 case 1 requests");
    }

    // (2) claim window expired but cache usable: old value shown immediately,
    // a background refresh is claimed and spawned within the 15s window.
    {
        let case = RenderDir::new(2);
        case.write_settings(true);
        case.write_cache(
            serde_json::to_vec(&json!({
                "fetchedAt": NOW - 10_000,
                "nextFetchAt": NOW - 1,
                "limits": [{"displayName": "Fable", "percent": 60, "resetsAt": null, "isActive": true}]
            }))
            .unwrap(),
        );
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::from_millis(1200));
        let (output, elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("60%"), "S-03 case 2 text: {text}");
        assert!(
            elapsed < Duration::from_millis(500),
            "S-03 case 2 elapsed: {elapsed:?}"
        );
        assert!(
            poll_until(Duration::from_secs(2), || server.requests().len() == 1),
            "S-03 case 2 expected exactly one request, got {}",
            server.requests().len()
        );
        let requests = server.requests();
        let recorded_bytes = requests[0]
            .cache_bytes_at_request
            .as_ref()
            .expect("S-03 case 2 recorded cache bytes at request time");
        let recorded: Value =
            serde_json::from_slice(recorded_bytes).expect("S-03 case 2 recorded cache parses");
        assert_eq!(
            recorded["nextFetchAt"],
            json!(NOW + 15_000),
            "S-03 case 2 claim window"
        );
    }

    // (3) no cache, immediate response: cold-start wait ends as soon as the
    // refresh writes a fetched cache.
    {
        let case = RenderDir::new(3);
        case.write_settings(true);
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let (output, elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("Fable"), "S-03 case 3 text: {text}");
        assert!(text.contains("65%"), "S-03 case 3 text: {text}");
        assert!(
            elapsed < Duration::from_millis(700),
            "S-03 case 3 elapsed: {elapsed:?}"
        );
        assert_eq!(server.requests().len(), 1, "S-03 case 3 requests");
    }

    // (4) no cache, delayed response: cold-start wait times out at ~800ms and
    // shows the placeholder, without killing the child.
    {
        let case = RenderDir::new(4);
        case.write_settings(true);
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::from_millis(1200));
        let (output, elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("--"), "S-03 case 4 text: {text}");
        assert!(
            elapsed >= Duration::from_millis(800),
            "S-03 case 4 elapsed: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1_500),
            "S-03 case 4 elapsed: {elapsed:?}"
        );
        assert!(
            poll_until(Duration::from_secs(3), || server.requests().len() == 1),
            "S-03 case 4 expected exactly one request eventually, got {}",
            server.requests().len()
        );
    }

    // (5) rows without limitModel: usage is never consulted, no directory,
    // no request.
    {
        let case = RenderDir::new(5);
        case.write_settings(false);
        let server = FakeServer::new(case.cache_path());
        let (output, _elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        assert!(output.status.success(), "S-03 case 5 render failed: {output:?}");
        assert!(
            !case.usage_dir_exists(),
            "S-03 case 5 usage dir must not exist"
        );
        assert_eq!(server.requests().len(), 0, "S-03 case 5 requests");
    }

    // (6) someone else's live claim (fetchedAt null, nextFetchAt in future):
    // poll the cache file, never spawn or request.
    {
        let case = RenderDir::new(6);
        case.write_settings(true);
        case.write_cache(
            serde_json::to_vec(&json!({
                "fetchedAt": null,
                "nextFetchAt": NOW + 10_000,
                "limits": []
            }))
            .unwrap(),
        );
        let before = case.cache_bytes();
        let server = FakeServer::new(case.cache_path());
        let (output, elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("--"), "S-03 case 6 text: {text}");
        assert!(
            elapsed >= Duration::from_millis(800),
            "S-03 case 6 elapsed: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1_500),
            "S-03 case 6 elapsed: {elapsed:?}"
        );
        assert_eq!(server.requests().len(), 0, "S-03 case 6 requests");
        assert_eq!(
            case.cache_bytes(),
            before,
            "S-03 case 6 cache bytes unchanged"
        );
    }

    // (7) someone else's expired claim (fetchedAt null, nextFetchAt in past):
    // this render claims and refreshes for real.
    {
        let case = RenderDir::new(7);
        case.write_settings(true);
        case.write_cache(
            serde_json::to_vec(&json!({
                "fetchedAt": null,
                "nextFetchAt": NOW - 1,
                "limits": []
            }))
            .unwrap(),
        );
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let (output, _elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("65%"), "S-03 case 7 text: {text}");
        assert_eq!(server.requests().len(), 1, "S-03 case 7 requests");
        assert!(
            poll_until(Duration::from_secs(2), || {
                let cache = case.cache();
                cache["fetchedAt"] == json!(NOW) && cache["nextFetchAt"] == json!(NOW + 300_000)
            }),
            "S-03 case 7 final cache did not settle: {:?}",
            case.cache()
        );
    }

    // (8) corrupt cache file: treated as unavailable, claimed and refreshed.
    {
        let case = RenderDir::new(8);
        case.write_settings(true);
        case.write_cache(b"{not json".to_vec());
        let server = FakeServer::new(case.cache_path());
        server.set_response(200, vec![], FIXTURE, Duration::ZERO);
        let (output, _elapsed) =
            run_render(&case.path, &server.url(), &case.token_file(), &stdin_json);
        let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(text.contains("65%"), "S-03 case 8 text: {text}");
        assert_eq!(server.requests().len(), 1, "S-03 case 8 requests");
        assert!(
            poll_until(Duration::from_secs(2), || {
                match serde_json::from_slice::<Value>(&case.cache_bytes()) {
                    Ok(v) => {
                        v.get("fetchedAt").is_some()
                            && v.get("nextFetchAt").is_some()
                            && v.get("limits").is_some()
                    }
                    Err(_) => false,
                }
            }),
            "S-03 case 8 final cache did not become a valid schema"
        );
    }
}
