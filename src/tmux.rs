use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Tmux {
    socket_args: Vec<String>,
    config: PathBuf,
}

impl Tmux {
    pub fn herdr_tabshell(plugin_root: impl AsRef<Path>) -> Self {
        Self {
            socket_args: vec!["-L".to_owned(), "herdr-tabshell".to_owned()],
            config: plugin_root.as_ref().join("tmux.conf"),
        }
    }

    #[cfg(test)]
    pub fn with_socket_path(socket_path: impl AsRef<Path>, config: impl Into<PathBuf>) -> Self {
        Self {
            socket_args: vec![
                "-S".to_owned(),
                socket_path.as_ref().to_string_lossy().into_owned(),
            ],
            config: config.into(),
        }
    }

    fn run(&self, args: &[&str]) -> io::Result<String> {
        let mut command = Command::new("tmux");
        command.args(&self.socket_args);
        if args.first() == Some(&"new-session") {
            command.arg("-f").arg(&self.config);
        }
        let output = command.args(args).output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    pub fn has_session(&self, session: &str) -> bool {
        let target = format!("={session}");
        self.run(&["has-session", "-t", &target]).is_ok()
    }

    pub fn ensure_window(
        &self,
        target: &TabTarget,
        tab_windows: bool,
        env: &[(String, String)],
    ) -> io::Result<String> {
        let label = if target.label.is_empty() {
            &target.tab_id
        } else {
            &target.label
        };
        let exact_session = format!("={}", target.session);
        let session_pane = format!("{exact_session}:");
        let session_exists = self.has_session(&target.session);
        let mut window_id = if session_exists {
            String::new()
        } else {
            self.create_session(target, label, env)?
        };

        if !tab_windows {
            let window_id =
                self.run(&["display-message", "-p", "-t", &session_pane, "#{window_id}"])?;
            self.install_exit_hook(&target.session)?;
            return Ok(window_id);
        }

        if session_exists {
            let windows = self.run(&[
                "list-windows",
                "-t",
                &exact_session,
                "-F",
                "#{window_id} #{@herdr_tab}",
            ])?;
            window_id = match windows
                .lines()
                .filter_map(|line| line.split_once(' '))
                .find_map(|(id, tab_id)| (tab_id == target.tab_id).then(|| id.to_owned()))
            {
                Some(id) => id,
                None => self.create_window(target, label, env)?,
            };
        }

        self.run(&[
            "set-option",
            "-w",
            "-t",
            &window_id,
            "@herdr_tab",
            &target.tab_id,
        ])?;
        self.append_window_id(&session_pane, &window_id)?;
        self.run(&["rename-window", "-t", &window_id, label])?;
        self.run(&["select-window", "-t", &window_id])?;
        self.install_exit_hook(&target.session)?;
        Ok(window_id)
    }

    pub fn mark_shown(&self, session: &str, window_id: &str) -> io::Result<()> {
        let target = session.to_owned();
        // Tmux-native window changes do not update this Herdr-selected window marker.
        self.run(&["set-option", "-t", &target, "@tabshell_shown", window_id])?;
        Ok(())
    }

    fn install_exit_hook(&self, session: &str) -> io::Result<()> {
        let target = session.to_owned();
        let existing = self.run(&["show-hooks", "-t", &target, "window-unlinked"])?;
        if existing.contains("hook_window")
            && existing.contains("@tabshell_shown")
            && existing.contains("detach-client -s")
        {
            return Ok(());
        }
        let session_target = tmux_quote(session);
        let detach = format!("detach-client -s {session_target}");
        let command = format!(
            r##"if-shell -F "#{{==:#{{hook_window}},#{{@tabshell_shown}}}}" "{}""##,
            tmux_escape_double(&detach)
        );
        self.run(&["set-hook", "-a", "-t", &target, "window-unlinked", &command])?;
        Ok(())
    }

    fn create_session(
        &self,
        target: &TabTarget,
        label: &str,
        env: &[(String, String)],
    ) -> io::Result<String> {
        let mut args = vec![
            "new-session".to_owned(),
            "-d".to_owned(),
            "-s".to_owned(),
            target.session.clone(),
            "-n".to_owned(),
            label.to_owned(),
        ];
        if let Some(cwd) = &target.cwd {
            args.extend(["-c".to_owned(), cwd.clone()]);
        }
        append_env_args(&mut args, env);
        args.extend(["-P".to_owned(), "-F".to_owned(), "#{window_id}".to_owned()]);
        self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn create_window(
        &self,
        target: &TabTarget,
        label: &str,
        env: &[(String, String)],
    ) -> io::Result<String> {
        let session_target = format!("={}:", target.session);
        let mut args = vec![
            "new-window".to_owned(),
            "-d".to_owned(),
            "-t".to_owned(),
            session_target,
            "-n".to_owned(),
            label.to_owned(),
        ];
        if let Some(cwd) = &target.cwd {
            args.extend(["-c".to_owned(), cwd.clone()]);
        }
        append_env_args(&mut args, env);
        args.extend(["-P".to_owned(), "-F".to_owned(), "#{window_id}".to_owned()]);
        self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn append_window_id(&self, session: &str, window_id: &str) -> io::Result<()> {
        let current = self.run(&["show-options", "-v", "-q", "-t", session, "@tabshell_ids"])?;
        let mut ids = current
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !ids.iter().any(|id| id == window_id) {
            ids.push(window_id.to_owned());
        }
        let value = format!(" {} ", ids.join(" "));
        self.run(&["set-option", "-t", session, "@tabshell_ids", &value])?;
        Ok(())
    }
}

fn tmux_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        quoted.push_str(&format!("\\u{:08X}", character as u32));
    }
    quoted.push('"');
    quoted
}

fn tmux_escape_double(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(character, '\\' | '"') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn append_env_args(args: &mut Vec<String>, env: &[(String, String)]) {
    for (key, value) in env {
        args.extend(["-e".to_owned(), format!("{key}={value}")]);
    }
}

pub struct TabTarget {
    pub session: String,
    pub tab_id: String,
    pub label: String,
    pub cwd: Option<String>,
}

pub fn herdr_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(key, _)| key.starts_with("HERDR_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Lock;
    use std::fs;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, Instant};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    struct Server {
        tmux: Tmux,
        socket: PathBuf,
        config_dir: Option<PathBuf>,
    }

    impl Server {
        fn new() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let config_dir = PathBuf::from(format!("/tmp/hc-{}-{id}", std::process::id()));
            fs::create_dir_all(&config_dir).unwrap();
            fs::write(
                config_dir.join("server.conf"),
                "set -g default-shell /bin/sh\n",
            )
            .unwrap();
            Self::with_config_dir(config_dir)
        }

        fn with_config(config: PathBuf) -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let socket = PathBuf::from(format!("/tmp/ht-{}-{id}.sock", std::process::id()));
            Self {
                tmux: Tmux::with_socket_path(&socket, config),
                socket,
                config_dir: None,
            }
        }

        fn with_config_dir(config_dir: PathBuf) -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let socket = PathBuf::from(format!("/tmp/ht-{}-{id}.sock", std::process::id()));
            Self {
                tmux: Tmux::with_socket_path(&socket, config_dir.join("server.conf")),
                socket,
                config_dir: Some(config_dir),
            }
        }

        fn with_fish() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let config_dir = PathBuf::from(format!("/tmp/hf-{}-{id}", std::process::id()));
            fs::create_dir_all(&config_dir).unwrap();
            let fish = Command::new("which")
                .arg("fish")
                .output()
                .expect("fish lookup should run");
            assert!(fish.status.success(), "fish is required for hook tests");
            let fish = String::from_utf8_lossy(&fish.stdout).trim().to_owned();
            fs::write(
                config_dir.join("server.conf"),
                format!(
                    "set -g default-shell {fish}\nset -g default-command \"{fish} --no-config\"\nset -g detach-on-destroy on\n"
                ),
            )
            .unwrap();
            Self::with_config_dir(config_dir)
        }

        fn query(&self, args: &[&str]) -> String {
            self.tmux.run(args).unwrap()
        }

        fn target(tab_id: &str, label: &str) -> TabTarget {
            TabTarget {
                session: "ws".to_owned(),
                tab_id: tab_id.to_owned(),
                label: label.to_owned(),
                cwd: None,
            }
        }

        fn count_windows(&self) -> usize {
            self.query(&["list-windows", "-t", "=ws", "-F", "#{window_id}"])
                .lines()
                .count()
        }

        fn tab_shell_ids(&self) -> String {
            self.query(&[
                "display-message",
                "-p",
                "-t",
                "=ws:",
                "BEGIN#{@tabshell_ids}END",
            ])
        }

        fn windows(&self) -> Vec<String> {
            self.query(&["list-windows", "-t", "=ws", "-F", "#{window_id}"])
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.tmux.run(&["kill-server"]);
            let _ = fs::remove_file(&self.socket);
            if let Some(dir) = &self.config_dir {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    fn lock_path() -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(format!("/tmp/ht-{}-{id}.lock", std::process::id()))
    }

    fn tab_ids(server: &Server) -> Vec<String> {
        server
            .query(&["list-windows", "-t", "=ws", "-F", "#{@herdr_tab}"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn threads_ensure_one(server: Arc<Server>, cold: bool) {
        let lock = lock_path();
        let ready = Arc::new(Barrier::new(5));
        let mut workers = Vec::new();
        for _ in 0..4 {
            let server = Arc::clone(&server);
            let lock = lock.clone();
            let ready = Arc::clone(&ready);
            workers.push(thread::spawn(move || {
                ready.wait();
                let _lock = Lock::acquire(lock).unwrap();
                server
                    .tmux
                    .ensure_window(&Server::target("shared", "Shared"), true, &[])
                    .unwrap();
            }));
        }
        ready.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(server.count_windows(), if cold { 1 } else { 2 });
        assert_eq!(
            tab_ids(&server)
                .iter()
                .filter(|id| id.as_str() == "shared")
                .count(),
            1
        );
        if cold {
            assert_eq!(
                server
                    .query(&["list-sessions", "-F", "#{session_name}"])
                    .lines()
                    .count(),
                1
            );
        }
        let _ = fs::remove_file(lock);
    }

    #[test]
    fn first_press_and_second_tab_create_and_bind_windows() {
        let server = Server::new();
        let mut first_target = Server::target("tab-1", "Alpha");
        first_target.cwd = Some(env!("CARGO_MANIFEST_DIR").to_owned());
        let first = server.tmux.ensure_window(&first_target, true, &[]).unwrap();
        assert_eq!(server.count_windows(), 1);
        assert_eq!(
            server.query(&["show-options", "-w", "-v", "-t", &first, "@herdr_tab"]),
            "tab-1"
        );
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &first, "#{window_name}"]),
            "Alpha"
        );
        assert_eq!(
            server.query(&[
                "display-message",
                "-p",
                "-t",
                &first,
                "#{pane_current_path}"
            ]),
            env!("CARGO_MANIFEST_DIR")
        );
        assert_eq!(server.tab_shell_ids(), format!("BEGIN {first} END"));

        let second = server
            .tmux
            .ensure_window(&Server::target("tab-2", "Beta"), true, &[])
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(server.count_windows(), 2);
        assert_eq!(
            server.tab_shell_ids(),
            format!("BEGIN {first} {second} END")
        );
    }

    #[test]
    fn return_rename_and_duplicate_labels_keep_tab_identity() {
        let server = Server::new();
        let first = server
            .tmux
            .ensure_window(&Server::target("tab-1", "Alpha"), true, &[])
            .unwrap();
        let second = server
            .tmux
            .ensure_window(&Server::target("tab-2", "Beta"), true, &[])
            .unwrap();
        let returned = server
            .tmux
            .ensure_window(&Server::target("tab-1", "Alpha"), true, &[])
            .unwrap();
        assert_eq!(returned, first);
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &first, "#{window_active}"]),
            "1"
        );
        assert_eq!(server.count_windows(), 2);
        assert_eq!(
            server.tab_shell_ids(),
            format!("BEGIN {first} {second} END")
        );

        let renamed = server
            .tmux
            .ensure_window(&Server::target("tab-1", "Renamed"), true, &[])
            .unwrap();
        assert_eq!(renamed, first);
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &first, "#{window_name}"]),
            "Renamed"
        );

        let duplicate = server
            .tmux
            .ensure_window(&Server::target("tab-3", "Beta"), true, &[])
            .unwrap();
        assert_ne!(duplicate, first);
        assert_ne!(duplicate, second);
        assert_eq!(server.count_windows(), 3);
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &duplicate, "#{window_name}"]),
            "Beta"
        );
    }

    #[test]
    fn warm_concurrent_presses_share_one_window() {
        let server = Arc::new(Server::new());
        server
            .tmux
            .ensure_window(&Server::target("base", "Base"), true, &[])
            .unwrap();
        threads_ensure_one(server, false);
    }

    #[test]
    fn cold_concurrent_presses_share_one_session_and_window() {
        threads_ensure_one(Arc::new(Server::new()), true);
    }

    #[test]
    fn tab_windows_false_returns_current_window_without_binding() {
        let server = Server::new();
        let created = server
            .tmux
            .ensure_window(&Server::target("first", "Alpha"), false, &[])
            .unwrap();
        assert_eq!(server.count_windows(), 1);
        assert!(server
            .query(&[
                "show-options",
                "-w",
                "-q",
                "-v",
                "-t",
                &created,
                "@herdr_tab"
            ])
            .is_empty());
        assert!(server
            .query(&["show-options", "-q", "-v", "-t", "=ws:", "@tabshell_ids"])
            .is_empty());
        let manual = server.query(&[
            "new-window",
            "-d",
            "-t",
            "=ws:",
            "-n",
            "Manual",
            "-P",
            "-F",
            "#{window_id}",
        ]);
        server.tmux.run(&["select-window", "-t", &manual]).unwrap();
        let returned = server
            .tmux
            .ensure_window(&Server::target("ignored", "Beta"), false, &[])
            .unwrap();
        assert_eq!(returned, manual);
        assert_eq!(server.count_windows(), 2);
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &manual, "#{window_name}"]),
            "Manual"
        );
    }

    #[test]
    fn forwarded_environment_reaches_session_and_new_window() {
        let server = Server::new();
        let session_env = vec![("HERDR_TEST_X".to_owned(), "session".to_owned())];
        server
            .tmux
            .ensure_window(&Server::target("first", "Alpha"), true, &session_env)
            .unwrap();
        assert_eq!(
            server.query(&["show-environment", "-t", "=ws", "HERDR_TEST_X"]),
            "HERDR_TEST_X=session"
        );

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let output_path = PathBuf::from(format!("/tmp/ht-env-{}-{id}", std::process::id()));
        let _ = fs::remove_file(&output_path);
        let window_env = vec![("HERDR_TEST_X".to_owned(), "window".to_owned())];
        let second = server
            .tmux
            .ensure_window(&Server::target("second", "Beta"), true, &window_env)
            .unwrap();
        let command = format!("printf '%s' \"$HERDR_TEST_X\" > {}", output_path.display());
        server.query(&["send-keys", "-t", &second, &command, "Enter"]);
        let deadline = Instant::now() + Duration::from_secs(3);
        let output = loop {
            if let Ok(value) = fs::read_to_string(&output_path) {
                break value;
            }
            assert!(
                Instant::now() < deadline,
                "window did not write its forwarded environment"
            );
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(output, "window");
        let _ = fs::remove_file(output_path);
    }

    #[test]
    fn label_and_session_are_passed_as_single_arguments_and_empty_label_uses_tab_id() {
        let server = Server::new();
        let mut target = Server::target("tab-id", "label with spaces");
        target.session = "session with spaces".to_owned();
        let id = server.tmux.ensure_window(&target, true, &[]).unwrap();
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &id, "#{window_name}"]),
            "label with spaces"
        );
        target.label.clear();
        let next = server.tmux.ensure_window(&target, true, &[]).unwrap();
        assert_eq!(next, id);
        assert_eq!(
            server.query(&["display-message", "-p", "-t", &id, "#{window_name}"]),
            "tab-id"
        );
    }

    #[test]
    fn optional_fish_window_command_runs_when_available() {
        let fish = Command::new("which")
            .arg("fish")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
        let Some(fish) = fish else {
            return;
        };
        let server = Server::new();
        server
            .tmux
            .ensure_window(&Server::target("base", "Base"), true, &[])
            .unwrap();
        let default_command = format!("{fish} --no-config");
        server
            .tmux
            .run(&["set-option", "-g", "default-command", &default_command])
            .unwrap();
        let id = server
            .tmux
            .ensure_window(&Server::target("fish", "Fish"), true, &[])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if server.query(&[
                "display-message",
                "-p",
                "-t",
                &id,
                "#{pane_current_command}",
            ]) == "fish"
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "fish did not start in the new window"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    struct ControlClient(std::process::Child);

    impl Drop for ControlClient {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    fn attach_control_client(server: &Server, session: &str) -> ControlClient {
        let target = format!("={session}");
        let child = ControlClient(
            Command::new("tmux")
                .args(&server.tmux.socket_args)
                .args(["-C", "attach-session", "-t", &target])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let clients = server.query(&["list-clients", "-F", "#{client_session}"]);
            if clients.lines().any(|line| line == session) {
                return child;
            }
            assert!(Instant::now() < deadline, "control client did not attach");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn assert_client_stays_attached(server: &Server, client: &mut ControlClient) {
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            assert!(
                client.0.try_wait().unwrap().is_none(),
                "control client detached unexpectedly"
            );
            assert!(
                server
                    .query(&["list-clients", "-F", "#{client_session}"])
                    .lines()
                    .any(|session| !session.is_empty()),
                "control client disappeared"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn assert_client_detaches(client: &mut ControlClient) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = client.0.try_wait().unwrap() {
                assert!(status.success(), "control attach exited with {status}");
                return;
            }
            assert!(Instant::now() < deadline, "control client did not detach");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_for_window_exit(server: &Server, window_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !server.windows().iter().any(|id| id == window_id) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("window {window_id} did not exit");
    }

    #[test]
    fn exit_hook_detaches_only_when_the_shown_tab_window_unlinks() {
        let server = Server::with_fish();
        let shown = server
            .tmux
            .ensure_window(&Server::target("shown-tab", "Shown"), true, &[])
            .unwrap();
        let hidden = server
            .tmux
            .ensure_window(&Server::target("hidden-tab", "Hidden"), true, &[])
            .unwrap();
        let plain = server.query(&[
            "new-window",
            "-d",
            "-t",
            "=ws",
            "-n",
            "Plain",
            "-P",
            "-F",
            "#{window_id}",
        ]);
        let sentinel = server.query(&[
            "new-window",
            "-d",
            "-t",
            "=ws",
            "-n",
            "Sentinel",
            "-P",
            "-F",
            "#{window_id}",
        ]);
        server.tmux.mark_shown("ws", &shown).unwrap();
        server.query(&["select-window", "-t", &shown]);
        let mut client = attach_control_client(&server, "ws");

        for background in [&hidden, &plain] {
            server.query(&["send-keys", "-t", background, "exit", "Enter"]);
            wait_for_window_exit(&server, background);
            assert_client_stays_attached(&server, &mut client);
            assert!(server.windows().contains(&sentinel));
        }

        server.query(&["send-keys", "-t", &shown, "exit", "Enter"]);
        wait_for_window_exit(&server, &shown);
        assert_client_detaches(&mut client);

        let killed = server
            .tmux
            .ensure_window(&Server::target("killed-tab", "Killed"), true, &[])
            .unwrap();
        server.tmux.mark_shown("ws", &killed).unwrap();
        server.query(&["select-window", "-t", &killed]);
        let mut client = attach_control_client(&server, "ws");
        server.query(&["kill-window", "-t", &killed]);
        wait_for_window_exit(&server, &killed);
        assert_client_detaches(&mut client);
    }

    #[test]
    fn exit_hook_installs_once_and_quotes_literal_session_names() {
        let server = Server::with_fish();
        let mut target = Server::target("special-tab", "Special");
        target.session = "ws space '$HOME; quote\"".to_owned();
        server.query(&["new-session", "-d", "-s", &target.session, "-n", "Sentinel"]);
        server.query(&[
            "set-hook",
            "-t",
            &target.session,
            "window-unlinked",
            "display-message -p custom-hook",
        ]);
        let window = server.tmux.ensure_window(&target, true, &[]).unwrap();
        server.tmux.ensure_window(&target, true, &[]).unwrap();
        let hooks = server.query(&["show-hooks", "-t", &target.session, "window-unlinked"]);
        assert_eq!(hooks.matches("@tabshell_shown").count(), 1, "{hooks}");
        assert!(hooks.contains("custom-hook"), "{hooks}");

        server.tmux.mark_shown(&target.session, &window).unwrap();
        server.query(&["select-window", "-t", &window]);
        let mut client = attach_control_client(&server, &target.session);
        server.query(&["kill-window", "-t", &window]);
        assert_client_detaches(&mut client);
    }

    #[test]
    fn config_expansion_probe() {
        if std::env::var_os("HERDR_CONFIG_PROBE_CHILD").is_none() {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let dir = PathBuf::from(format!("/tmp/hc-{}-{id}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("tmux.conf"), "set -g @probe yes\n").unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tmux::tests::config_expansion_probe",
                    "--nocapture",
                ])
                .env("HERDR_CONFIG_PROBE_CHILD", "1")
                .env("HERDR_PLUGIN_CONFIG_DIR", &dir)
                .env("SHELL", "/bin/sh")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .unwrap();
            let _ = fs::remove_dir_all(&dir);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let server =
            Server::with_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmux.conf"));
        let plugin_config_dir = std::env::var("HERDR_PLUGIN_CONFIG_DIR").unwrap();
        let env = vec![("HERDR_PLUGIN_CONFIG_DIR".to_owned(), plugin_config_dir)];
        server
            .tmux
            .ensure_window(&Server::target("probe", "Probe"), true, &env)
            .unwrap();
        assert_eq!(server.query(&["show-options", "-gqv", "@probe"]), "yes");
        assert_eq!(server.query(&["show-options", "-g", "mouse"]), "mouse on");
        let bindings = server.query(&["list-keys", "-T", "prefix"]);
        assert!(!bindings
            .lines()
            .any(|line| line.split_whitespace().any(|field| field == "C-s")));

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let empty_dir = PathBuf::from(format!("/tmp/hc-empty-{}-{id}", std::process::id()));
        fs::create_dir_all(&empty_dir).unwrap();
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", &empty_dir);
        let without_user_file =
            Server::with_config(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmux.conf"));
        without_user_file
            .tmux
            .ensure_window(
                &Server::target("probe", "Probe"),
                true,
                &[(
                    "HERDR_PLUGIN_CONFIG_DIR".to_owned(),
                    empty_dir.to_string_lossy().into_owned(),
                )],
            )
            .unwrap();
        let _ = fs::remove_dir_all(empty_dir);
    }
}
