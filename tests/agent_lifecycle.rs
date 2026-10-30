//! Real IPC/process tests use an isolated profile directory; no input or HID writes.
use mk::{
    host::{HostAction, HostBinding, HostConfig, HostProfile, HostState},
    service::{Client, Event, Job},
};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Agent {
    process: Child,
    dir: PathBuf,
}
impl Agent {
    fn spawn(dir: PathBuf) -> Self {
        let process = Command::new(env!("CARGO_BIN_EXE_mk-agent"))
            .env("MK_DATA_DIR", &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self { process, dir }
    }
    fn client(&self) -> Client {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(client) = Client::connect_at(&self.dir) {
                return client;
            }
            assert!(Instant::now() < until, "agent not ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn wait(&mut self) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.process.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < until, "agent did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
#[test]
fn agent_owns_profiles_survives_clients_and_never_auto_enables_input() {
    let dir = std::env::temp_dir().join(format!(
        "mk-ipc-test-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let legacy=br#"{"profiles":[{"name":"web","application":"","bindings":[{"trigger":104,"action":{"type":"Open","value":"example.com"}}]}]}"#;
    std::fs::write(dir.join("host-profiles.json"), legacy).unwrap();
    let mut agent = Agent::spawn(dir.clone());
    let client = agent.client();
    let Event::HostStatus(initial) = client.call(Job::Status).unwrap() else {
        panic!()
    };
    assert!(!initial.enabled);
    assert_eq!(initial.state, HostState::Stopped);
    assert_eq!(
        std::fs::read(dir.join("host-profiles.json")).unwrap(),
        legacy
    );
    let Event::Profiles { config, revision } = client.call(Job::Profiles).unwrap() else {
        panic!()
    };
    assert_eq!(revision, 0);
    assert!(matches!(config.action("", 104), Some(HostAction::Web(_))));
    // A second launch and independent client must reuse the one running agent.
    let mut duplicate = Agent::spawn(dir.clone());
    duplicate.wait();
    let other = agent.client();
    let Event::HostStatus(status) = other.call(Job::Status).unwrap() else {
        panic!()
    };
    assert_eq!(status.instance, initial.instance);
    let config = HostConfig {
        display_names: [(104, "Mở bài giảng".into()), (0x7700, "Lời giải".into())].into(),
        profiles: vec![HostProfile {
            name: "changed".into(),
            application: String::new(),
            bindings: vec![HostBinding {
                trigger: 104,
                action: HostAction::Web("example.org".into()),
            }],
        }],
    };
    let Event::ProfilesSaved { revision, .. } = client
        .call(Job::UpdateProfiles {
            expected_revision: 0,
            config: config.clone(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revision, 1);
    assert!(matches!(
        other
            .call(Job::UpdateProfiles {
                expected_revision: 0,
                config: HostConfig::default()
            })
            .unwrap(),
        Event::Error(_, false)
    ));
    drop(client);
    drop(other);
    let reopened = agent.client();
    let Event::ScreenDraft {
        revision: 0,
        mut layout,
        ..
    } = reopened.call(Job::LoadScreen { layer: 0 }).unwrap()
    else {
        panic!()
    };
    layout.tiles[0].text = "Lời giải".into();
    let Event::ScreenPreview {
        revision: 1, png, ..
    } = reopened
        .call(Job::RenderScreen {
            layer: 0,
            expected_revision: 0,
            layout: layout.clone(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert!(png.starts_with(b"\x89PNG"));
    assert!(matches!(
        reopened
            .call(Job::RenderScreen {
                layer: 0,
                expected_revision: 0,
                layout
            })
            .unwrap(),
        Event::Error(_, false)
    ));
    let Event::Profiles {
        config: loaded,
        revision,
    } = reopened.call(Job::Profiles).unwrap()
    else {
        panic!()
    };
    assert_eq!(revision, 1);
    assert_eq!(loaded.profiles[0].name, "changed");
    assert_eq!(loaded.display_name(104), Some("Mở bài giảng"));
    assert_eq!(loaded.display_name(0x7700), Some("Lời giải"));
    assert!(matches!(
        reopened.call(Job::Read).unwrap(),
        Event::Error(_, true)
    )); // no selected unit; never guess by VID/PID
    assert!(matches!(
        reopened.call(Job::StopAgent).unwrap(),
        Event::Done(_)
    ));
    agent.wait();
    assert!(reopened.call(Job::Status).is_err());
    let mut replacement = Agent::spawn(dir.clone());
    let fresh = replacement.client();
    let Event::ScreenDraft {
        revision: 1,
        layout,
        ..
    } = fresh.call(Job::LoadScreen { layer: 0 }).unwrap()
    else {
        panic!()
    };
    assert_eq!(layout.tiles[0].text, "Lời giải");
    assert!(
        reopened
            .call(Job::UpdateProfiles {
                expected_revision: 1,
                config
            })
            .is_err()
    ); // old process epoch cannot mutate the replacement
    let Event::HostStatus(status) = fresh.call(Job::Status).unwrap() else {
        panic!()
    };
    assert_ne!(status.instance, initial.instance);
    assert!(!status.enabled);
    let Event::Profiles { config, revision } = fresh.call(Job::Profiles).unwrap() else {
        panic!()
    };
    assert_eq!(revision, 1);
    assert_eq!(config.display_name(104), Some("Mở bài giảng"));
    assert_eq!(config.display_name(0x7700), Some("Lời giải"));
    fresh.call(Job::StopAgent).unwrap();
    replacement.wait();
    std::fs::remove_dir_all(dir).unwrap();
}
