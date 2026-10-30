use mk::{
    host::{HostAction, HostBinding, HostConfig, HostProfile},
    keycodes, macros,
    model::Snapshot,
    protocol::{MockTransport, Protocol},
    storage,
};
fn sample() -> (Protocol<MockTransport>, Snapshot) {
    let mut p = Protocol::new(MockTransport::new());
    let s = p.read_snapshot(p.io.snapshot.device.clone()).unwrap();
    (p, s)
}
#[test]
fn keycodes_roundtrip_and_modifiers() {
    for code in [
        0, 1, 4, 41, 104, 0x7703, 0x5201, 0x5223, 0x7e07, 0x0806, 0xdead,
    ] {
        assert_eq!(keycodes::parse(&keycodes::label(code)).unwrap(), code);
    }
    assert_eq!(keycodes::parse("CTRL+SHIFT+ESC").unwrap(), 0x0329);
    assert!(keycodes::parse("CTRL+A+B").is_err());
}
#[test]
fn direct_changes_are_acknowledged_before_becoming_current() {
    let (mut p, baseline) = sample();
    let mut editor = mk::editor::Editor::default();
    editor.replace(baseline.clone()).unwrap();
    let mut target = baseline.clone();
    target.set_binding(0, 2, 104).unwrap();
    let request = editor
        .begin(target.clone(), "Đã gán F13".into())
        .unwrap()
        .unwrap();
    assert!(editor.current().unwrap().same_content(&baseline));
    assert!(!editor.can_undo());
    assert!(editor.begin(target.clone(), "Duplicate".into()).is_err());
    let result = p.apply(&request.before, &request.after).unwrap();
    editor.confirm(result.snapshot).unwrap();
    assert!(editor.current().unwrap().same_content(&target));
    assert!(editor.can_undo());
    let undo = editor.begin_undo().unwrap().unwrap();
    let result = p.apply(&undo.before, &undo.after).unwrap();
    editor.confirm(result.snapshot).unwrap();
    assert!(editor.current().unwrap().same_content(&baseline));
    assert!(!editor.can_undo());
}
#[test]
fn direct_write_failure_requires_read_and_never_replays() {
    let (_, baseline) = sample();
    let mut editor = mk::editor::Editor::default();
    editor.replace(baseline.clone()).unwrap();
    let mut target = baseline.clone();
    target.set_binding(1, 10, 115).unwrap();
    editor.begin(target.clone(), "Encoder".into()).unwrap();
    editor.fail();
    assert!(editor.current().unwrap().same_content(&baseline));
    assert!(editor.begin(target.clone(), "Retry".into()).is_err());
    assert!(!editor.can_undo());
    editor.replace(baseline.clone()).unwrap();
    assert!(
        editor
            .begin(baseline, "No change".into())
            .unwrap()
            .is_none()
    );
    assert!(editor.history.is_empty());
}
#[test]
fn direct_write_wrong_readback_is_not_reported_saved() {
    let (_, baseline) = sample();
    let mut editor = mk::editor::Editor::default();
    editor.replace(baseline.clone()).unwrap();
    let mut target = baseline.clone();
    target.set_binding(0, 3, 115).unwrap();
    editor.begin(target, "F24".into()).unwrap();
    assert!(editor.confirm(baseline.clone()).is_err());
    assert!(editor.current().unwrap().same_content(&baseline));
    assert!(editor.history.is_empty());
    assert!(!editor.can_undo());
}
#[test]
fn friendly_macro_steps_preserve_existing_bytes_and_support_shortcuts() {
    use mk::macro_steps::{self, Step};
    let bytes = macros::encode("hello{KC_ENT}{KC_LGUI,KC_C}{200}\\{").unwrap();
    let steps = macro_steps::decode(&bytes).unwrap();
    assert_eq!(
        macros::encode(&macro_steps::encode(&steps).unwrap()).unwrap(),
        bytes
    );
    let shortcut = [Step {
        kind: 1,
        text: "⌘ + Shift + C".into(),
    }];
    assert_eq!(
        macros::encode(&macro_steps::encode(&shortcut).unwrap()).unwrap(),
        macros::encode("{KC_LSFT,KC_LGUI,KC_C}").unwrap()
    );
    assert!(
        macro_steps::encode(&[Step {
            kind: 0,
            text: "Tiếng Việt".into()
        }])
        .is_err()
    );
    assert!(
        macro_steps::encode(&[Step {
            kind: 3,
            text: "Shift".into()
        }])
        .is_err()
    );
}
#[test]
fn friendly_macro_names_roundtrip_every_basic_key() {
    for code in 2..=255 {
        let bytes = vec![1, 1, code];
        let steps = mk::macro_steps::decode(&bytes).unwrap();
        let encoded =
            mk::macro_steps::encode(&steps).unwrap_or_else(|e| panic!("{}: {e:#}", steps[0].text));
        assert_eq!(macros::encode(&encoded).unwrap(), bytes);
    }
}
#[test]
fn editing_and_removing_macro_modifiers_keep_release_paired() {
    let bytes = macros::encode("{KC_RGUI,KC_C}").unwrap();
    let mut steps = mk::macro_steps::decode(&bytes).unwrap();
    mk::macro_steps::set_text(&mut steps, 0, "Ctrl".into()).unwrap();
    assert_eq!(
        macros::encode(&mk::macro_steps::encode(&steps).unwrap()).unwrap(),
        macros::encode("{KC_LCTL,KC_C}").unwrap()
    );
    mk::macro_steps::remove(&mut steps, 0).unwrap();
    assert_eq!(
        macros::encode(&mk::macro_steps::encode(&steps).unwrap()).unwrap(),
        macros::encode("{+KC_C}{-KC_C}").unwrap()
    );
}
#[test]
fn friendly_functions_have_human_names_and_device_limits() {
    let s = Snapshot::demo();
    assert_eq!(
        mk::presentation::describe(0x0806, Some(&s)).title,
        "Sao chép"
    );
    assert_eq!(
        mk::presentation::describe(0xa9, Some(&s)).title,
        "Tăng âm lượng"
    );
    assert_eq!(
        mk::presentation::describe(0x7e06, Some(&s)).title,
        "Bắt đầu tập trung"
    );
    assert_eq!(mk::presentation::catalog("", 1, 0, Some(&s)).len(), 24);
    for category in 0..4 {
        for f in mk::presentation::catalog("", category, 0, Some(&s)) {
            assert!(!f.title.contains("KC_"));
            assert!(!f.detail.contains("KC_"));
        }
    }
    let entries = mk::presentation::catalog("sao chép", 0, 0, Some(&s));
    assert!(!entries.is_empty());
    assert!(entries.iter().all(|f| f.title == "Sao chép"));
}
#[test]
fn function_catalog_is_complete_ordered_and_capacity_aware() {
    let keys = keycodes::choices("", 1, 4, 16);
    assert_eq!(keys.len(), 24);
    for (i, entry) in keys.iter().enumerate() {
        assert_eq!(entry.code, format!("KC_F{}", i + 1));
        assert_eq!(
            keycodes::parse(&entry.code).unwrap(),
            if i < 12 {
                58 + i as u16
            } else {
                104 + i as u16 - 12
            }
        );
    }
    let matches = keycodes::choices("âm lượng", 3, 4, 16);
    assert!(matches.iter().any(|x| x.code == "KC_VOLU"));
    assert!(matches.iter().any(|x| x.code == "KC_VOLD"));
    assert_eq!(keycodes::choices("F13", 1, 4, 16).len(), 1);
    assert_eq!(keycodes::choices("", 5, 2, 3).len(), 3);
    assert_eq!(keycodes::choices("", 6, 2, 3).len(), 12);
}
#[test]
fn macro_chord_delay_and_escaping() {
    let text = "abc{KC_ENT}{KC_LGUI,KC_C}{500}\\{";
    let bytes = macros::encode(text).unwrap();
    let decoded = macros::decode(&bytes).unwrap();
    assert_eq!(macros::encode(&decoded).unwrap(), bytes);
    assert!(bytes.ends_with(b"{"));
}
#[test]
fn macro_rejects_unicode_and_stuck_keys() {
    for s in [
        "Tiếng Việt",
        "{+KC_LCTL}x",
        "{99999}",
        "{KC_BAD}",
        "{",
        "{}",
        "{-KC_LSFT}",
    ] {
        assert!(macros::encode(s).is_err(), "{s}");
    }
}
#[test]
fn edit_one_macro_preserves_other_macro_bytes() {
    let mut b = vec![0; 120];
    let original = b"old\0\x01\x02\xe3\x01\x01\x06\x01\x03\xe3\0last\0";
    b[..original.len()].copy_from_slice(original);
    let before = macros::split(&b, 3).unwrap();
    let out = macros::replace(&b, 3, 0, "new{25}").unwrap();
    let after = macros::split(&out, 3).unwrap();
    assert_eq!(&before[1..], &after[1..]);
    assert_eq!(out.len(), b.len());
}
#[test]
fn macro_capacity_and_malformed_buffer() {
    assert!(macros::replace(&[0, 0, 0], 3, 0, "four").is_err());
    assert!(macros::split(&[1, 4, b'1'], 1).is_err());
    assert!(macros::split(&[1, 2], 1).is_err());
}
#[test]
fn snapshot_shape_and_device_guard() {
    let mut s = Snapshot::demo();
    s.device.product_id = 1;
    assert!(s.validate().is_err());
    let mut s = Snapshot::demo();
    s.keymap.pop();
    assert!(s.validate().is_err());
}
#[test]
fn reading_and_noop_apply_send_no_writes() {
    let (mut p, s) = sample();
    let result = p.apply(&s, &s).unwrap();
    assert_eq!(result.changes, 0);
    assert_eq!(p.io.writes, 0);
    assert!(result.backup.is_none());
}
#[test]
fn key_and_encoder_apply_preserve_rest() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.set_binding(2, 3, 4).unwrap();
    d.set_binding(1, 13, 41).unwrap();
    let result = p.apply(&s, &d).unwrap();
    assert_eq!(result.changes, 2);
    assert!(result.snapshot.same_content(&d));
    assert_eq!(s.macro_buffer, result.snapshot.macro_buffer);
    assert_eq!(result.snapshot.keymap[1], s.keymap[1]);
}
#[test]
fn refuses_live_drift_before_writes() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.set_binding(0, 2, 4).unwrap();
    p.io.snapshot.keymap[4] = 5;
    let e = p.apply(&s, &d).unwrap_err().to_string();
    assert!(e.contains("bên ngoài"));
    assert_eq!(p.io.writes, 0);
}
#[test]
fn disconnect_reports_partial_and_stops() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.set_binding(0, 2, 4).unwrap();
    d.set_binding(0, 3, 5).unwrap();
    p.io.fail_after = Some(1);
    let err = p.apply(&s, &d).unwrap_err().to_string();
    assert!(err.contains("một phần"));
    assert!(err.contains("Bản sao"));
    assert_eq!(p.io.writes, 1);
}
#[test]
fn invalid_target_rejected_before_write() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.layers = 5;
    assert!(p.apply(&s, &d).is_err());
    assert_eq!(p.io.writes, 0);
}
#[test]
fn backup_checksum_detects_tamper() {
    let s = Snapshot::demo();
    let p = std::env::temp_dir().join(format!("mk-test-backup-{}.json", std::process::id()));
    storage::save(&p, &s).unwrap();
    assert_eq!(storage::load(&p).unwrap(), s);
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    json["snapshot"]["keymap"][0] = serde_json::json!(4);
    std::fs::write(&p, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(storage::load(&p).is_err());
    std::fs::remove_file(p).unwrap();
}
#[test]
fn rapid_backups_never_share_a_filename_or_temporary_file() {
    let jobs: Vec<_> = (0..16)
        .map(|_| {
            std::thread::spawn(|| {
                let snapshot = Snapshot::demo();
                let path = storage::automatic_backup(&snapshot).unwrap();
                assert!(storage::load(&path).unwrap().same_content(&snapshot));
                path
            })
        })
        .collect();
    let paths: std::collections::HashSet<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(paths.len(), 16);
}
#[test]
fn settings_write_and_macro_name_readback() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.settings
        .iter_mut()
        .find(|x| x.key == "rgb_brightness")
        .unwrap()
        .value[0] = 120;
    d.macro_names[0] = "Copy".into();
    d.macro_buffer =
        macros::replace(&d.macro_buffer, d.macro_count as usize, 3, "hello{KC_ENT}").unwrap();
    let r = p.apply(&s, &d).unwrap();
    assert!(r.snapshot.same_content(&d));
}
#[test]
fn host_profile_routing_is_in_memory() {
    let cfg = HostConfig {
        display_names: Default::default(),
        profiles: vec![
            HostProfile {
                name: "Default".into(),
                application: "".into(),
                bindings: vec![HostBinding {
                    trigger: 104,
                    action: HostAction::Text("Việt Nam".into()),
                }],
            },
            HostProfile {
                name: "Editor".into(),
                application: "com.apple.TextEdit".into(),
                bindings: vec![HostBinding {
                    trigger: 104,
                    action: HostAction::Text("Editor".into()),
                }],
            },
        ],
    };
    cfg.validate().unwrap();
    assert!(
        matches!(cfg.action("com.apple.TextEdit",104),Some(HostAction::Text(x)) if x=="Editor")
    );
    assert!(matches!(cfg.action("other",104),Some(HostAction::Text(x)) if x=="Việt Nam"));
}
#[test]
fn incomplete_backup_rejected_before_any_write() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.settings.pop();
    d.set_binding(0, 2, 4).unwrap();
    assert!(p.apply(&s, &d).is_err());
    assert_eq!(p.io.writes, 0);
}
#[test]
fn all_changes_validated_before_first_write() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.set_binding(0, 2, 4).unwrap();
    d.settings
        .iter_mut()
        .find(|x| x.key == "rgb_effect")
        .unwrap()
        .value[0] = 255;
    assert!(p.apply(&s, &d).is_err());
    assert_eq!(p.io.writes, 0);
}
#[test]
fn simulated_backup_cannot_target_real_device() {
    let (mut p, mut s) = sample();
    s.simulated = false;
    let d = Snapshot::demo();
    assert!(p.apply(&s, &d).is_err());
    assert_eq!(p.io.writes, 0);
}
#[test]
fn local_gif_is_standard_and_bounded() {
    let img = image::RgbaImage::from_pixel(160, 80, image::Rgba([42, 132, 80, 255]));
    let bytes = mk::media::encode_frames(&[(img, 10)]).unwrap();
    assert!(bytes.starts_with(b"GIF89a"));
    let out = mk::media::preview(&bytes).unwrap();
    assert_eq!((out.width(), out.height()), (160, 80));
    assert!(bytes.len() < mk::media::MAX_BYTES);
}
#[test]
fn nonexistent_layers_and_macros_cannot_be_written() {
    let (mut p, s) = sample();
    for code in [0x521f, 0x523f, 0x777f] {
        let mut d = s.clone();
        d.set_binding(0, 2, code).unwrap();
        assert!(p.apply(&s, &d).is_err());
    }
    assert_eq!(p.io.writes, 0);
}
#[test]
fn unsupported_screen_enum_rejected_before_any_write() {
    let (mut p, s) = sample();
    let mut d = s.clone();
    d.set_binding(0, 2, 4).unwrap();
    d.settings
        .iter_mut()
        .find(|x| x.key == "screen")
        .unwrap()
        .value[0] = 4;
    assert!(p.apply(&s, &d).is_err());
    assert_eq!(p.io.writes, 0);
}
#[test]
fn host_macro_checks_all_key_support_before_execution() {
    assert!(mk::host::validate_macro("{KC_LGUI,KC_C}{200}{KC_F5}").is_ok());
    assert!(mk::host::validate_macro("{+KC_LGUI}{KC_POWER}{-KC_LGUI}").is_err());
}
