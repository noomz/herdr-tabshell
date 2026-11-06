use crate::config::Config;
use crate::herdr;
use crate::lock::Lock;
use crate::tmux::{herdr_env, TabTarget, Tmux};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait HerdrApi {
    fn call(&mut self, method: &str, params: Value) -> io::Result<Value>;
}

struct SocketHerdr;

impl HerdrApi for SocketHerdr {
    fn call(&mut self, method: &str, params: Value) -> io::Result<Value> {
        herdr::call(method, params)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FloatState {
    Absent,
    Hidden(String),
    Shown(String),
}

struct Invocation {
    plugin_id: String,
    workspace_id: String,
    tab_id: String,
    tab_label: String,
    focused_pane_cwd: Option<String>,
    state_dir: PathBuf,
    plugin_root: PathBuf,
    config: Config,
}

impl Invocation {
    fn from_env() -> io::Result<Self> {
        let context_json = required_env("HERDR_PLUGIN_CONTEXT_JSON")?;
        let context: Value = serde_json::from_str(&context_json).map_err(|error| {
            io::Error::other(format!("invalid HERDR_PLUGIN_CONTEXT_JSON: {error}"))
        })?;
        let workspace_id = required_env("HERDR_WORKSPACE_ID")?;
        let tab_id = required_env("HERDR_TAB_ID")?;
        let plugin_id = required_env("HERDR_PLUGIN_ID")?;
        let state_dir = PathBuf::from(required_env("HERDR_PLUGIN_STATE_DIR")?);
        let plugin_root = PathBuf::from(required_env("HERDR_PLUGIN_ROOT")?);
        Ok(Self {
            plugin_id,
            workspace_id,
            tab_id,
            tab_label: context["tab_label"].as_str().unwrap_or_default().to_owned(),
            focused_pane_cwd: context["focused_pane_cwd"].as_str().map(str::to_owned),
            state_dir,
            plugin_root,
            config: Config::load(),
        })
    }

    fn target(&self) -> TabTarget {
        TabTarget {
            session: self.workspace_id.clone(),
            tab_id: self.tab_id.clone(),
            label: self.tab_label.clone(),
            cwd: self.focused_pane_cwd.clone(),
        }
    }
}

fn required_env(name: &str) -> io::Result<String> {
    std::env::var(name)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("{name} not set")))
}

fn workspace_state_path(state_dir: &Path, workspace_id: &str) -> PathBuf {
    let encoded = workspace_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    state_dir.join(format!("float-{encoded}.json"))
}

fn load_saved_id(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))?;
            Ok(value["float_id"].as_str().map(str::to_owned))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn save_id(path: &Path, float_id: &str) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec(&json!({"float_id": float_id}))?)?;
    fs::rename(temp, path)
}

fn forget_id(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn current_state<H: HerdrApi>(
    api: &mut H,
    path: &Path,
    workspace_id: &str,
) -> io::Result<FloatState> {
    let Some(saved_id) = load_saved_id(path)? else {
        return Ok(FloatState::Absent);
    };
    let result = api.call("float.list", json!({}))?;
    let floats = result["floats"]
        .as_array()
        .ok_or_else(|| io::Error::other("float.list response missing floats array"))?;
    let Some(float) = floats.iter().find(|float| {
        let workspace_matches = float
            .get("workspace_id")
            .is_none_or(|id| id.as_str() == Some(workspace_id));
        float["float_id"].as_str() == Some(saved_id.as_str()) && workspace_matches
    }) else {
        forget_id(path)?;
        return Ok(FloatState::Absent);
    };
    match float["visibility"].as_str() {
        Some("shown") => Ok(FloatState::Shown(saved_id)),
        Some("hidden") => Ok(FloatState::Hidden(saved_id)),
        visibility => Err(io::Error::other(format!(
            "float.list returned invalid visibility {visibility:?}"
        ))),
    }
}

fn float_id_from_open(result: &Value) -> io::Result<&str> {
    result["float"]["float_id"]
        .as_str()
        .ok_or_else(|| io::Error::other("plugin.pane.open response missing float.float_id"))
}

fn pane_env(invocation: &Invocation) -> Value {
    let mut env = serde_json::Map::new();
    env.insert(
        "TABSHELL_SESSION".to_owned(),
        json!(invocation.workspace_id),
    );
    env.insert("TABSHELL_TAB_ID".to_owned(), json!(invocation.tab_id));
    env.insert("TABSHELL_TAB_LABEL".to_owned(), json!(invocation.tab_label));
    if let Some(cwd) = &invocation.focused_pane_cwd {
        env.insert("TABSHELL_CWD".to_owned(), json!(cwd));
    }
    Value::Object(env)
}

fn open_params(invocation: &Invocation) -> Value {
    json!({
        "plugin_id": invocation.plugin_id,
        "entrypoint": "shell",
        "placement": "float",
        "workspace_id": invocation.workspace_id,
        "focus": true,
        "hide_on_blur": true,
        "width": invocation.config.width.to_json(),
        "height": invocation.config.height.to_json(),
        "env": pane_env(invocation),
    })
}

fn toggle_with<H: HerdrApi>(invocation: &Invocation, api: &mut H, tmux: &Tmux) -> io::Result<()> {
    fs::create_dir_all(&invocation.state_dir)?;
    let _lock = Lock::acquire(invocation.state_dir.join("lock"))?;
    let path = workspace_state_path(&invocation.state_dir, &invocation.workspace_id);
    match current_state(api, &path, &invocation.workspace_id)? {
        FloatState::Shown(float_id) => {
            api.call("float.hide", json!({"float_id": float_id}))?;
        }
        FloatState::Hidden(float_id) => {
            let window_id = tmux.ensure_window(
                &invocation.target(),
                invocation.config.tab_windows,
                &herdr_env(),
            )?;
            tmux.mark_shown(&invocation.workspace_id, &window_id)?;
            api.call("float.show", json!({"float_id": float_id, "focus": true}))?;
        }
        FloatState::Absent => {
            let window_id = tmux.ensure_window(
                &invocation.target(),
                invocation.config.tab_windows,
                &herdr_env(),
            )?;
            tmux.mark_shown(&invocation.workspace_id, &window_id)?;
            let opened = api.call("plugin.pane.open", open_params(invocation))?;
            save_id(&path, float_id_from_open(&opened)?)?;
        }
    }
    Ok(())
}

fn shell_argv(session: &str) -> Vec<OsString> {
    ["-L", "herdr-tabshell", "attach-session", "-t"]
        .into_iter()
        .map(OsString::from)
        .chain([OsString::from(format!("={session}"))])
        .collect()
}

fn shell_entry() -> io::Result<()> {
    let session = required_env("TABSHELL_SESSION")?;
    let state_dir = PathBuf::from(required_env("HERDR_PLUGIN_STATE_DIR")?);
    fs::create_dir_all(&state_dir)?;
    let _lock = Lock::acquire(state_dir.join("lock"))?;
    let tmux = Tmux::herdr_tabshell(".");
    if !tmux.has_session(&session) {
        let plugin_root = PathBuf::from(required_env("HERDR_PLUGIN_ROOT")?);
        let tab_id = required_env("TABSHELL_TAB_ID")?;
        let target = TabTarget {
            session: session.clone(),
            tab_id,
            label: std::env::var("TABSHELL_TAB_LABEL").unwrap_or_default(),
            cwd: std::env::var("TABSHELL_CWD").ok(),
        };
        let window_id = Tmux::herdr_tabshell(plugin_root).ensure_window(
            &target,
            Config::load().tab_windows,
            &herdr_env(),
        )?;
        tmux.mark_shown(&session, &window_id)?;
    }
    drop(_lock);
    let error = Command::new("tmux").args(shell_argv(&session)).exec();
    Err(error)
}

pub fn run_toggle() -> io::Result<()> {
    let invocation = Invocation::from_env()?;
    let tmux = Tmux::herdr_tabshell(&invocation.plugin_root);
    toggle_with(&invocation, &mut SocketHerdr, &tmux)
}

pub fn run_shell() -> io::Result<()> {
    shell_entry()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeHerdr {
        responses: Vec<Value>,
        calls: Vec<(String, Value)>,
    }

    impl HerdrApi for FakeHerdr {
        fn call(&mut self, method: &str, params: Value) -> io::Result<Value> {
            self.calls.push((method.to_owned(), params));
            Ok(self.responses.remove(0))
        }
    }

    fn invocation(root: &Path, workspace: &str) -> Invocation {
        Invocation {
            plugin_id: "herdr-tabshell".to_owned(),
            workspace_id: workspace.to_owned(),
            tab_id: "tab-id".to_owned(),
            tab_label: "dev".to_owned(),
            focused_pane_cwd: Some("/repo".to_owned()),
            state_dir: root.join("state"),
            plugin_root: root.to_owned(),
            config: Config::default(),
        }
    }

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tabshell-command-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    static TEMP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn absent_opens_float_with_required_options_and_saves_id() {
        let root = temp_dir();
        let mut invocation = invocation(&root, "workspace-a");
        invocation.config.width = crate::config::Size::Cells(90);
        invocation.config.height = crate::config::Size::Percent(75);
        let mut fake = FakeHerdr {
            responses: vec![json!({"float": {"float_id": "float-1"}})],
            calls: Vec::new(),
        };
        let mut server = TestServer::new();
        toggle_with(&invocation, &mut fake, &server.tmux).unwrap();
        assert_eq!(fake.calls.len(), 1);
        assert_eq!(fake.calls[0].0, "plugin.pane.open");
        assert_eq!(
            fake.calls[0].1,
            json!({
                "plugin_id": "herdr-tabshell",
                "entrypoint": "shell",
                "placement": "float",
                "workspace_id": "workspace-a",
                "focus": true,
                "hide_on_blur": true,
                "width": 90,
                "height": "75%",
                "env": {
                    "TABSHELL_SESSION": "workspace-a",
                    "TABSHELL_TAB_ID": "tab-id",
                    "TABSHELL_TAB_LABEL": "dev",
                    "TABSHELL_CWD": "/repo"
                }
            })
        );
        let path = workspace_state_path(&invocation.state_dir, "workspace-a");
        assert_eq!(load_saved_id(&path).unwrap().as_deref(), Some("float-1"));
        server.stop();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_float_selects_current_tab_then_shows_with_focus() {
        let root = temp_dir();
        let invocation = invocation(&root, "workspace-a");
        let path = workspace_state_path(&invocation.state_dir, "workspace-a");
        save_id(&path, "float-1").unwrap();
        let mut fake = FakeHerdr {
            responses: vec![
                json!({"floats": [{"float_id": "float-1", "visibility": "hidden", "workspace_id": "workspace-a"}]}),
                json!({}),
            ],
            calls: Vec::new(),
        };
        let server = TestServer::new();
        toggle_with(&invocation, &mut fake, &server.tmux).unwrap();
        assert_eq!(fake.calls[0], ("float.list".to_owned(), json!({})));
        assert_eq!(
            fake.calls[1],
            (
                "float.show".to_owned(),
                json!({"float_id": "float-1", "focus": true})
            )
        );
        assert_eq!(
            server.query(&[
                "display-message",
                "-p",
                "-t",
                "=workspace-a:",
                "#{@tabshell_shown}"
            ]),
            "@0"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shown_float_hides_without_touching_tmux() {
        let root = temp_dir();
        let invocation = invocation(&root, "workspace-a");
        let path = workspace_state_path(&invocation.state_dir, "workspace-a");
        save_id(&path, "float-1").unwrap();
        let mut fake = FakeHerdr {
            responses: vec![
                json!({"floats": [{"float_id": "float-1", "visibility": "shown"}]}),
                json!({}),
            ],
            calls: Vec::new(),
        };
        let server = TestServer::new();
        toggle_with(&invocation, &mut fake, &server.tmux).unwrap();
        assert_eq!(
            fake.calls,
            vec![
                ("float.list".to_owned(), json!({})),
                ("float.hide".to_owned(), json!({"float_id": "float-1"})),
            ]
        );
        assert!(!server.tmux.has_session("workspace-a"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_saved_float_is_forgotten_and_reopened() {
        let root = temp_dir();
        let invocation = invocation(&root, "workspace-a");
        let path = workspace_state_path(&invocation.state_dir, "workspace-a");
        save_id(&path, "stale-float").unwrap();
        let mut fake = FakeHerdr {
            responses: vec![
                json!({"floats": []}),
                json!({"float": {"float_id": "fresh-float"}}),
            ],
            calls: Vec::new(),
        };
        let server = TestServer::new();
        toggle_with(&invocation, &mut fake, &server.tmux).unwrap();
        assert_eq!(fake.calls[0].0, "float.list");
        assert_eq!(fake.calls[1].0, "plugin.pane.open");
        assert_eq!(
            load_saved_id(&path).unwrap().as_deref(),
            Some("fresh-float")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_state_files_do_not_share_float_ids() {
        let root = temp_dir();
        let invocation_a = invocation(&root, "workspace-a");
        let path_a = workspace_state_path(&invocation_a.state_dir, "workspace-a");
        save_id(&path_a, "float-a").unwrap();
        let invocation_b = invocation(&root, "workspace-b");
        let mut fake = FakeHerdr {
            responses: vec![json!({"float": {"float_id": "float-b"}})],
            calls: Vec::new(),
        };
        let server = TestServer::new();
        toggle_with(&invocation_b, &mut fake, &server.tmux).unwrap();
        assert_eq!(fake.calls[0].0, "plugin.pane.open");
        assert_eq!(fake.calls[0].1["workspace_id"], "workspace-b");
        assert_eq!(load_saved_id(&path_a).unwrap().as_deref(), Some("float-a"));
        assert_eq!(
            load_saved_id(&workspace_state_path(
                &invocation_b.state_dir,
                "workspace-b"
            ))
            .unwrap()
            .as_deref(),
            Some("float-b")
        );
        let _ = fs::remove_dir_all(root);
    }

    struct TestServer {
        tmux: Tmux,
        socket: PathBuf,
        config_dir: PathBuf,
    }

    impl TestServer {
        fn new() -> Self {
            let socket = std::env::temp_dir().join(format!(
                "tabshell-test-{}-{}.sock",
                std::process::id(),
                TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let config_dir = std::env::temp_dir().join(format!(
                "tabshell-test-config-{}-{}",
                std::process::id(),
                TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&config_dir).unwrap();
            let fish = Command::new("which").arg("fish").output().unwrap();
            assert!(fish.status.success(), "fish is required for tmux tests");
            let fish = String::from_utf8_lossy(&fish.stdout).trim().to_owned();
            fs::write(
                config_dir.join("tmux.conf"),
                format!(
                    "set -g default-shell {fish}\nset -g default-command \"{fish} --no-config\"\n"
                ),
            )
            .unwrap();
            Self {
                tmux: Tmux::with_socket_path(&socket, config_dir.join("tmux.conf")),
                socket,
                config_dir,
            }
        }

        fn stop(&mut self) {
            let _ = Command::new("tmux")
                .arg("-S")
                .arg(&self.socket)
                .arg("kill-server")
                .output();
            let _ = fs::remove_file(&self.socket);
            let _ = fs::remove_dir_all(&self.config_dir);
        }

        fn query(&self, args: &[&str]) -> String {
            let output = Command::new("tmux")
                .arg("-S")
                .arg(&self.socket)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop();
        }
    }

    #[test]
    fn shell_argv_attaches_to_exact_session() {
        assert_eq!(
            shell_argv("workspace id"),
            [
                "-L",
                "herdr-tabshell",
                "attach-session",
                "-t",
                "=workspace id"
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
    }
}
