//! JNI bindings exposing Obscura headless browser, Chrome DevTools Protocol (CDP),
//! and Model Context Protocol (MCP) servers to Android.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};
use jni::JNIEnv;
use serde::{Deserialize, Serialize};

// ── Global Server Handles ──────────────────────────────────────────────────

struct ServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    port: u16,
    host: String,
}

static CDP_HANDLE: Mutex<Option<ServerHandle>> = Mutex::new(None);
static MCP_HANDLE: Mutex<Option<ServerHandle>> = Mutex::new(None);
static CDP_RUNNING: AtomicBool = AtomicBool::new(false);
static MCP_RUNNING: AtomicBool = AtomicBool::new(false);

// ── Android Logging & Stdio Redirection ───────────────────────────────────

#[allow(dead_code)]
fn android_log(_prio: i32, msg: &str) {
    #[cfg(target_os = "android")]
    {
        extern "C" {
            fn __android_log_write(
                prio: i32,
                tag: *const std::os::raw::c_char,
                text: *const std::os::raw::c_char,
            ) -> i32;
        }
        use std::ffi::CString;
        if let (Ok(tag), Ok(text)) = (CString::new("ObscuraNative"), CString::new(msg)) {
            unsafe {
                __android_log_write(_prio, tag.as_ptr(), text.as_ptr());
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("[ObscuraNative] {msg}");
    }
}

#[cfg(target_os = "android")]
fn redirect_stdio_to_logcat() {
    unsafe {
        let mut pfd = [0i32; 2];
        if libc::pipe(pfd.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(pfd[1], libc::STDOUT_FILENO);
        libc::dup2(pfd[1], libc::STDERR_FILENO);
        libc::close(pfd[1]);

        let read_fd = pfd[0];
        thread::Builder::new()
            .name("obscura-stdio-pipe".to_string())
            .spawn(move || {
                use std::io::{BufRead, BufReader};
                use std::os::unix::io::FromRawFd;
                let file = std::fs::File::from_raw_fd(read_fd);
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            if !l.is_empty() {
                                android_log(5, &format!("[NativeStderr] {l}"));
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .ok();
    }
}

/// JNI lifecycle hook called automatically when libobscura_android.so is loaded.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(
    _vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    #[cfg(target_os = "android")]
    redirect_stdio_to_logcat();

    std::panic::set_hook(Box::new(|info| {
        let msg = format!("RUST PANIC: {info}");
        android_log(6, &msg);
        let backtrace = std::backtrace::Backtrace::force_capture();
        android_log(6, &format!("RUST PANIC BACKTRACE:\n{backtrace}"));
    }));

    android_log(
        4,
        "=== JNI_OnLoad: libobscura_android.so initialized successfully ===",
    );
    jni::sys::JNI_VERSION_1_6
}

// ── Options Structure ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct BrowserOptions {
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    stealth: Option<bool>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    settle_ms: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn parse_options_json(options_str: Option<&str>) -> BrowserOptions {
    if let Some(s) = options_str {
        if !s.trim().is_empty() {
            if let Ok(opts) = serde_json::from_str::<BrowserOptions>(s) {
                return opts;
            }
        }
    }
    BrowserOptions::default()
}

fn get_optional_jstring(env: &mut JNIEnv, js: &JString) -> Option<String> {
    if js.is_null() {
        return None;
    }
    env.get_string(js).ok().map(|s| s.into())
}

// ── Core Browser Operations ────────────────────────────────────────────────

/// Fetch HTML from URL with default settings (backward-compatible).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeFetchHtml(
    env: JNIEnv,
    class: JClass,
    url: JString,
) -> jstring {
    let null_opts = unsafe { JString::from_raw(std::ptr::null_mut()) };
    Java_com_obscura_app_ObscuraBridge_nativeFetchHtmlWithOptions(env, class, url, null_opts)
}

/// Fetch rendered HTML with customizable options (proxy, stealth, settle_ms, user_agent).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeFetchHtmlWithOptions(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
    options_json: JString,
) -> jstring {
    let url_str: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(e) => {
            return env
                .new_string(format!("Error: Invalid URL string: {e}"))
                .map(|s| s.into_raw())
                .unwrap_or(std::ptr::null_mut());
        }
    };

    let opts_str = get_optional_jstring(&mut env, &options_json);
    let opts = parse_options_json(opts_str.as_deref());

    android_log(4, &format!("nativeFetchHtml: fetching '{url_str}'"));

    let target_url = url_str.clone();
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Tokio runtime creation failed: {e}"))?;

        rt.block_on(async move {
            let mut builder = obscura::Browser::builder();
            if let Some(proxy) = opts.proxy {
                builder = builder.proxy(proxy);
            }
            if let Some(stealth) = opts.stealth {
                builder = builder.stealth(stealth);
            }
            if let Some(ua) = opts.user_agent {
                builder = builder.user_agent(ua);
            }

            let browser = builder
                .build()
                .map_err(|e| format!("Browser initialization failed: {e}"))?;

            let mut page = browser
                .new_page()
                .await
                .map_err(|e| format!("new_page failed: {e}"))?;

            page.goto(&target_url)
                .await
                .map_err(|e| format!("Navigation failed: {e}"))?;

            if let Some(settle_ms) = opts.settle_ms {
                if settle_ms > 0 {
                    page.settle(settle_ms).await;
                }
            }

            Ok::<String, String>(page.content())
        })
    }));

    let output = match panic_result {
        Ok(Ok(html)) => html,
        Ok(Err(err)) => format!("Error: {err}"),
        Err(panic_payload) => {
            let msg = extract_panic_message(panic_payload);
            android_log(6, &format!("nativeFetchHtml caught panic: {msg}"));
            format!("Error (Panic): {msg}")
        }
    };

    env.new_string(output)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Execute arbitrary JavaScript inside a rendered web page and return the result as JSON.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeEvaluateJs(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
    script: JString,
    options_json: JString,
) -> jstring {
    let url_str = match env.get_string(&url) {
        Ok(s) => String::from(s),
        Err(e) => return env.new_string(format!("Error: {e}")).unwrap().into_raw(),
    };
    let script_str = match env.get_string(&script) {
        Ok(s) => String::from(s),
        Err(e) => return env.new_string(format!("Error: {e}")).unwrap().into_raw(),
    };

    let opts_str = get_optional_jstring(&mut env, &options_json);
    let opts = parse_options_json(opts_str.as_deref());

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Tokio runtime creation failed: {e}"))?;

        rt.block_on(async move {
            let mut builder = obscura::Browser::builder();
            if let Some(proxy) = opts.proxy {
                builder = builder.proxy(proxy);
            }
            if let Some(stealth) = opts.stealth {
                builder = builder.stealth(stealth);
            }
            if let Some(ua) = opts.user_agent {
                builder = builder.user_agent(ua);
            }

            let browser = builder.build().map_err(|e| format!("Browser build failed: {e}"))?;
            let mut page = browser.new_page().await.map_err(|e| format!("new_page failed: {e}"))?;

            if !url_str.is_empty() && url_str != "about:blank" {
                page.goto(&url_str).await.map_err(|e| format!("Navigation failed: {e}"))?;
            }

            if let Some(settle_ms) = opts.settle_ms {
                if settle_ms > 0 {
                    page.settle(settle_ms).await;
                }
            }

            let val = page.evaluate(&script_str);
            Ok::<String, String>(serde_json::to_string(&val).unwrap_or_else(|_| "null".to_string()))
        })
    }));

    let output = match panic_result {
        Ok(Ok(json)) => json,
        Ok(Err(err)) => format!("{{\"error\": \"{err}\"}}"),
        Err(payload) => format!("{{\"error\": \"Panic: {}\"}}", extract_panic_message(payload)),
    };

    env.new_string(output).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Extract clean, readable plain text from a URL.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeExtractText(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
    options_json: JString,
) -> jstring {
    let url_str = match env.get_string(&url) {
        Ok(s) => String::from(s),
        Err(e) => return env.new_string(format!("Error: {e}")).unwrap().into_raw(),
    };
    let opts_str = get_optional_jstring(&mut env, &options_json);
    let opts = parse_options_json(opts_str.as_deref());

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Tokio runtime creation failed: {e}"))?;

        rt.block_on(async move {
            let mut builder = obscura::Browser::builder();
            if let Some(proxy) = opts.proxy {
                builder = builder.proxy(proxy);
            }
            if let Some(stealth) = opts.stealth {
                builder = builder.stealth(stealth);
            }
            if let Some(ua) = opts.user_agent {
                builder = builder.user_agent(ua);
            }

            let browser = builder.build().map_err(|e| format!("Browser build failed: {e}"))?;
            let mut page = browser.new_page().await.map_err(|e| format!("new_page failed: {e}"))?;

            page.goto(&url_str).await.map_err(|e| format!("Navigation failed: {e}"))?;
            if let Some(settle_ms) = opts.settle_ms {
                if settle_ms > 0 {
                    page.settle(settle_ms).await;
                }
            }

            let val = page.evaluate("document.body ? document.body.innerText : (document.documentElement ? document.documentElement.innerText : '')");
            Ok::<String, String>(val.as_str().unwrap_or_default().to_string())
        })
    }));

    let output = match panic_result {
        Ok(Ok(text)) => text,
        Ok(Err(err)) => format!("Error: {err}"),
        Err(payload) => format!("Error (Panic): {}", extract_panic_message(payload)),
    };

    env.new_string(output).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Convert rendered web page into clean Markdown (optimized for AI/LLM prompts).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeExtractMarkdown(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
    options_json: JString,
) -> jstring {
    let url_str = match env.get_string(&url) {
        Ok(s) => String::from(s),
        Err(e) => return env.new_string(format!("Error: {e}")).unwrap().into_raw(),
    };
    let opts_str = get_optional_jstring(&mut env, &options_json);
    let opts = parse_options_json(opts_str.as_deref());

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Tokio runtime creation failed: {e}"))?;

        rt.block_on(async move {
            let mut builder = obscura::Browser::builder();
            if let Some(proxy) = opts.proxy {
                builder = builder.proxy(proxy);
            }
            if let Some(stealth) = opts.stealth {
                builder = builder.stealth(stealth);
            }
            if let Some(ua) = opts.user_agent {
                builder = builder.user_agent(ua);
            }

            let browser = builder.build().map_err(|e| format!("Browser build failed: {e}"))?;
            let mut page = browser.new_page().await.map_err(|e| format!("new_page failed: {e}"))?;

            page.goto(&url_str).await.map_err(|e| format!("Navigation failed: {e}"))?;
            if let Some(settle_ms) = opts.settle_ms {
                if settle_ms > 0 {
                    page.settle(settle_ms).await;
                }
            }

            let val = page.evaluate(obscura_browser::HTML_TO_MARKDOWN_JS);
            Ok::<String, String>(val.as_str().unwrap_or_default().to_string())
        })
    }));

    let output = match panic_result {
        Ok(Ok(md)) => md,
        Ok(Err(err)) => format!("Error: {err}"),
        Err(payload) => format!("Error (Panic): {}", extract_panic_message(payload)),
    };

    env.new_string(output).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

// ── CDP (Chrome DevTools Protocol) ─────────────────────────────────────────

/// Start CDP server on specified port (backward-compatible).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStartCdp(
    env: JNIEnv,
    class: JClass,
    port: jint,
) -> jboolean {
    Java_com_obscura_app_ObscuraBridge_nativeStartCdpWithOptions(
        env,
        class,
        port,
        unsafe { JString::from_raw(std::ptr::null_mut()) },
        unsafe { JString::from_raw(std::ptr::null_mut()) },
        0,
        unsafe { JString::from_raw(std::ptr::null_mut()) },
    )
}

/// Start CDP server with custom options (host, proxy, stealth, user-agent).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStartCdpWithOptions(
    mut env: JNIEnv,
    _class: JClass,
    port: jint,
    host: JString,
    proxy: JString,
    stealth: jboolean,
    user_agent: JString,
) -> jboolean {
    let port_u16 = if port > 0 && port <= 65535 {
        port as u16
    } else {
        9222
    };

    let host_str = get_optional_jstring(&mut env, &host).unwrap_or_else(|| "127.0.0.1".to_string());
    let proxy_str = get_optional_jstring(&mut env, &proxy);
    let ua_str = get_optional_jstring(&mut env, &user_agent);
    let is_stealth = stealth != 0;

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut handle_lock = CDP_HANDLE.lock().unwrap();
        if handle_lock.is_some() || CDP_RUNNING.load(Ordering::SeqCst) {
            android_log(5, "nativeStartCdp: server is already running");
            return 1;
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server_host = host_str.clone();

        *handle_lock = Some(ServerHandle {
            shutdown_tx,
            port: port_u16,
            host: host_str.clone(),
        });
        CDP_RUNNING.store(true, Ordering::SeqCst);

        let builder = thread::Builder::new().name("obscura-cdp".to_string());
        let spawn_res = builder.spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    android_log(6, &format!("nativeStartCdp: failed to create Tokio runtime: {e}"));
                    CDP_RUNNING.store(false, Ordering::SeqCst);
                    if let Ok(mut lock) = CDP_HANDLE.lock() {
                        *lock = None;
                    }
                    return;
                }
            };

            rt.block_on(async move {
                android_log(
                    4,
                    &format!("nativeStartCdp: listening on ws://{server_host}:{port_u16}"),
                );
                tokio::select! {
                    srv_res = obscura_cdp::start_with_host_and_security(
                        port_u16,
                        &server_host,
                        proxy_str,
                        is_stealth,
                        ua_str,
                        true,
                        None,
                    ) => {
                        if let Err(e) = srv_res {
                            android_log(6, &format!("nativeStartCdp server error: {e}"));
                        }
                    }
                    _ = shutdown_rx => {
                        android_log(4, "nativeStartCdp: shutdown signal received; terminating CDP server");
                    }
                }
            });

            CDP_RUNNING.store(false, Ordering::SeqCst);
            if let Ok(mut lock) = CDP_HANDLE.lock() {
                *lock = None;
            }
            android_log(4, "nativeStartCdp: CDP server thread exited cleanly");
        });

        if let Err(e) = spawn_res {
            android_log(6, &format!("nativeStartCdp: failed to spawn server thread: {e}"));
            CDP_RUNNING.store(false, Ordering::SeqCst);
            *handle_lock = None;
            return 0;
        }

        1
    }));

    match res {
        Ok(code) => code,
        Err(p) => {
            android_log(6, &format!("nativeStartCdp panic: {}", extract_panic_message(p)));
            0
        }
    }
}

/// Stop the CDP server cleanly and release socket/threads.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStopCdp(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    android_log(4, "nativeStopCdp: stopping CDP server");
    let mut handle_lock = match CDP_HANDLE.lock() {
        Ok(h) => h,
        Err(_) => return 0,
    };

    if let Some(handle) = handle_lock.take() {
        let _ = handle.shutdown_tx.send(());
        CDP_RUNNING.store(false, Ordering::SeqCst);
        1
    } else {
        CDP_RUNNING.store(false, Ordering::SeqCst);
        1
    }
}

/// Check if the CDP server is currently active.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeIsCdpRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if CDP_RUNNING.load(Ordering::SeqCst) {
        1
    } else {
        0
    }
}

/// Get WebSocket endpoint URL for CDP connection (e.g. for Playwright or Puppeteer).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeGetCdpEndpoint(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let endpoint = if let Ok(lock) = CDP_HANDLE.lock() {
        if let Some(ref handle) = *lock {
            format!("ws://{}:{}/devtools/browser", handle.host, handle.port)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    env.new_string(endpoint)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

// ── MCP (Model Context Protocol) ───────────────────────────────────────────

/// Start MCP HTTP/SSE server on specified port.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStartMcp(
    env: JNIEnv,
    class: JClass,
    port: jint,
) -> jboolean {
    Java_com_obscura_app_ObscuraBridge_nativeStartMcpWithOptions(
        env,
        class,
        port,
        unsafe { JString::from_raw(std::ptr::null_mut()) },
        unsafe { JString::from_raw(std::ptr::null_mut()) },
        0,
        unsafe { JString::from_raw(std::ptr::null_mut()) },
    )
}

/// Start MCP HTTP/SSE server with custom options (host, proxy, stealth, user-agent).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStartMcpWithOptions(
    mut env: JNIEnv,
    _class: JClass,
    port: jint,
    host: JString,
    proxy: JString,
    stealth: jboolean,
    user_agent: JString,
) -> jboolean {
    let port_u16 = if port > 0 && port <= 65535 {
        port as u16
    } else {
        8080
    };

    let host_str = get_optional_jstring(&mut env, &host).unwrap_or_else(|| "127.0.0.1".to_string());
    let proxy_str = get_optional_jstring(&mut env, &proxy);
    let ua_str = get_optional_jstring(&mut env, &user_agent);
    let is_stealth = stealth != 0;

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut handle_lock = MCP_HANDLE.lock().unwrap();
        if handle_lock.is_some() || MCP_RUNNING.load(Ordering::SeqCst) {
            android_log(5, "nativeStartMcp: MCP server is already running");
            return 1;
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server_host = host_str.clone();

        *handle_lock = Some(ServerHandle {
            shutdown_tx,
            port: port_u16,
            host: host_str.clone(),
        });
        MCP_RUNNING.store(true, Ordering::SeqCst);

        let builder = thread::Builder::new().name("obscura-mcp".to_string());
        let spawn_res = builder.spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    android_log(6, &format!("nativeStartMcp: failed to create Tokio runtime: {e}"));
                    MCP_RUNNING.store(false, Ordering::SeqCst);
                    if let Ok(mut lock) = MCP_HANDLE.lock() {
                        *lock = None;
                    }
                    return;
                }
            };

            rt.block_on(async move {
                android_log(
                    4,
                    &format!("nativeStartMcp: listening on http://{server_host}:{port_u16}/sse"),
                );
                tokio::select! {
                    mcp_res = obscura_mcp::http::run(
                        server_host,
                        port_u16,
                        proxy_str,
                        ua_str,
                        is_stealth,
                    ) => {
                        if let Err(e) = mcp_res {
                            android_log(6, &format!("nativeStartMcp server error: {e}"));
                        }
                    }
                    _ = shutdown_rx => {
                        android_log(4, "nativeStartMcp: shutdown signal received; terminating MCP server");
                    }
                }
            });

            MCP_RUNNING.store(false, Ordering::SeqCst);
            if let Ok(mut lock) = MCP_HANDLE.lock() {
                *lock = None;
            }
            android_log(4, "nativeStartMcp: MCP server thread exited cleanly");
        });

        if let Err(e) = spawn_res {
            android_log(6, &format!("nativeStartMcp: failed to spawn server thread: {e}"));
            MCP_RUNNING.store(false, Ordering::SeqCst);
            *handle_lock = None;
            return 0;
        }

        1
    }));

    match res {
        Ok(code) => code,
        Err(p) => {
            android_log(6, &format!("nativeStartMcp panic: {}", extract_panic_message(p)));
            0
        }
    }
}

/// Stop the MCP server cleanly.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeStopMcp(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    android_log(4, "nativeStopMcp: stopping MCP server");
    let mut handle_lock = match MCP_HANDLE.lock() {
        Ok(h) => h,
        Err(_) => return 0,
    };

    if let Some(handle) = handle_lock.take() {
        let _ = handle.shutdown_tx.send(());
        MCP_RUNNING.store(false, Ordering::SeqCst);
        1
    } else {
        MCP_RUNNING.store(false, Ordering::SeqCst);
        1
    }
}

/// Check if the MCP server is currently active.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeIsMcpRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if MCP_RUNNING.load(Ordering::SeqCst) {
        1
    } else {
        0
    }
}

/// Get HTTP/SSE endpoint URL for MCP connection (e.g. for Claude Desktop, AI Agents).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeGetMcpEndpoint(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let endpoint = if let Ok(lock) = MCP_HANDLE.lock() {
        if let Some(ref handle) = *lock {
            format!("http://{}:{}/sse", handle.host, handle.port)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    env.new_string(endpoint)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Direct in-process execution of MCP tool calls (no HTTP network roundtrip needed).
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeExecuteMcpTool(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JString,
) -> jstring {
    let req_str = match env.get_string(&request_json) {
        Ok(s) => String::from(s),
        Err(e) => return env.new_string(format!("{{\"error\":\"{e}\"}}")).unwrap().into_raw(),
    };

    let panic_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Tokio runtime creation failed: {e}"))?;

        rt.block_on(async move {
            let (host, port) = {
                let lock = MCP_HANDLE.lock().unwrap();
                if let Some(ref handle) = *lock {
                    (handle.host.clone(), handle.port)
                } else {
                    return Err("MCP server is not running. Please start it via ObscuraBridge.startMcp(port) first.".to_string());
                }
            };

            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = tokio::net::TcpStream::connect(format!("{host}:{port}"))
                .await
                .map_err(|e| format!("Failed to connect to MCP server at {host}:{port}: {e}"))?;

            let body_bytes = req_str.as_bytes();
            let http_req = format!(
                "POST /message HTTP/1.1\r\n\
                Host: {host}:{port}\r\n\
                Content-Type: application/json\r\n\
                Content-Length: {}\r\n\
                Connection: close\r\n\
                \r\n",
                body_bytes.len()
            );

            stream.write_all(http_req.as_bytes()).await.map_err(|e| e.to_string())?;
            stream.write_all(body_bytes).await.map_err(|e| e.to_string())?;

            let mut response_buf = Vec::new();
            stream.read_to_end(&mut response_buf).await.map_err(|e| e.to_string())?;

            let response_str = String::from_utf8_lossy(&response_buf);
            if let Some(pos) = response_str.find("\r\n\r\n") {
                Ok(response_str[pos + 4..].to_string())
            } else {
                Ok(response_str.to_string())
            }
        })
    }));

    let output = match panic_res {
        Ok(Ok(json)) => json,
        Ok(Err(err)) => format!("{{\"error\":\"{err}\"}}"),
        Err(p) => format!("{{\"error\":\"Panic: {}\"}}", extract_panic_message(p)),
    };

    env.new_string(output).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

// ── Diagnostics & Engine Info ──────────────────────────────────────────────

#[derive(Serialize)]
struct EngineDiagnostics {
    version: &'static str,
    arch: &'static str,
    os: &'static str,
    cdp_running: bool,
    mcp_running: bool,
    cdp_endpoint: Option<String>,
    mcp_endpoint: Option<String>,
}

/// Get engine status and version metadata as JSON.
#[no_mangle]
pub extern "system" fn Java_com_obscura_app_ObscuraBridge_nativeGetEngineDiagnostics(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let cdp_ep = if let Ok(lock) = CDP_HANDLE.lock() {
        lock.as_ref().map(|h| format!("ws://{}:{}/devtools/browser", h.host, h.port))
    } else {
        None
    };

    let mcp_ep = if let Ok(lock) = MCP_HANDLE.lock() {
        lock.as_ref().map(|h| format!("http://{}:{}/sse", h.host, h.port))
    } else {
        None
    };

    let diag = EngineDiagnostics {
        version: env!("CARGO_PKG_VERSION"),
        arch: std::env::consts::ARCH,
        os: std::env::consts::OS,
        cdp_running: CDP_RUNNING.load(Ordering::SeqCst),
        mcp_running: MCP_RUNNING.load(Ordering::SeqCst),
        cdp_endpoint: cdp_ep,
        mcp_endpoint: mcp_ep,
    };

    let json = serde_json::to_string_pretty(&diag).unwrap_or_else(|_| "{}".to_string());
    env.new_string(json)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn extract_panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload".to_string()
    }
}

// ── ARM64 CPU Cache Invalidation Builtins ───────────────────────────────────

/// Clear CPU instruction cache on ARM64 / Android.
///
/// V8 JIT code generation invokes compiler-rt / libgcc builtins (`__clear_cache` or `_clear_cache`),
/// which are not exported by Android's Bionic libc. We provide exported C symbols so that dlopen
/// does not fail with "cannot locate symbol '_clear_cache'".
#[no_mangle]
pub unsafe extern "C" fn __clear_cache(start: *mut std::ffi::c_void, end: *mut std::ffi::c_void) {
    clear_cache_range(start, end);
}

#[no_mangle]
pub unsafe extern "C" fn _clear_cache(start: *mut std::ffi::c_void, end: *mut std::ffi::c_void) {
    clear_cache_range(start, end);
}

#[used]
static _KEEP_CLEAR_CACHE: [unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void); 2] = [
    __clear_cache,
    _clear_cache,
];

#[inline(always)]
unsafe fn clear_cache_range(start: *mut std::ffi::c_void, end: *mut std::ffi::c_void) {
    let beg = start as usize;
    let end = end as usize;
    if beg >= end {
        return;
    }

    #[cfg(target_arch = "aarch64")]
    {
        let ctr: usize;
        core::arch::asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack));

        let dsize = 4 << ((ctr >> 16) & 0xf);
        let isize = 4 << (ctr & 0xf);

        // 1. Clean data cache line by VA to PoU
        let mut dline = beg & !(dsize - 1);
        while dline < end {
            core::arch::asm!("dc civac, {}", in(reg) dline, options(nostack));
            dline += dsize;
        }

        // Barrier to ensure data cache clean is observed
        core::arch::asm!("dsb ish", options(nostack));

        // 2. Invalidate instruction cache line by VA to PoU
        let mut iline = beg & !(isize - 1);
        while iline < end {
            core::arch::asm!("ic ivau, {}", in(reg) iline, options(nostack));
            iline += isize;
        }

        // Final barriers to flush CPU pipeline
        core::arch::asm!("dsb ish", options(nostack));
        core::arch::asm!("isb", options(nostack));
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = beg;
        let _ = end;
    }
}
