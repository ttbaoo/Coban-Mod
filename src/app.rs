use crate::{FunctionView, IconChoice, MKTray, MainWindow, SettingField, StepView};
use anyhow::{Context, Result, bail, ensure};
use mk::{
    editor::{Change, Editor},
    host::{self, HostAction, HostBinding, HostConfig, HostProfile},
    macro_steps::{self, Step},
    macros,
    model::Snapshot,
    presentation,
    protocol::setting_specs,
    screen_layout::{self, ClockFace, ClockLayout, Content, DateFormat, IconWeight, Layout},
    service::{self, Event, Job},
    storage,
    transport::DeviceInfo,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc, sync::mpsc};
#[cfg(target_os = "macos")]
#[path = "app_macos.rs"]
mod macos;
#[derive(Clone)]
enum Undo {
    Device,
    Host(HostConfig),
}
#[derive(Default)]
struct State {
    devices: Vec<DeviceInfo>,
    active_path: Option<std::ffi::CString>,
    ever_connected: bool,
    editor: Editor,
    setting_keys: Vec<String>,
    steps: Vec<Step>,
    macro_view: usize,
    host_view: Option<(i32, i32)>,
    host: HostConfig,
    sender: Option<mpsc::Sender<Job>>,
    host_revision: u64,
    agent_instance: String,
    profiles_loaded: bool,
    poll_pending: bool,
    pending_host: Option<(HostConfig, String, bool)>,
    after_host_save: Option<Job>,
    after_device_write: Option<Job>,
    stop_after_response: bool,
    // The key service starts by itself once per agent instance/connection.
    auto_actions: bool,
    undo: Vec<Undo>,
    history: Vec<String>,
    pending_undo: bool,
    quit_after_save: bool,
    hide_after_save: bool,
    rendering: bool,
    screen_layout: Layout,
    screen_revision: u64,
    screen_layer: u8,
    pending_screen_layer: Option<u8>,
    // Manual inspection lasts until the device next changes layer.
    inspection_origin: Option<u8>,
    layer_monitoring: bool,
    screen_loaded: bool,
    screen_loading: bool,
    screen_rendering: bool,
    screen_applying: bool,
    screen_tile: usize,
    screen_asset_target: Option<bool>,
    apply_after_preview: bool,
    screen_preview_valid: bool,
    // Clock tiles: the minute last sent to the LCD, an in-flight tick and the upload lead.
    clock_shown: Option<chrono::NaiveDateTime>,
    clock_sending: Option<chrono::NaiveDateTime>,
    clock_refreshing: Option<chrono::NaiveDateTime>,
    clock_paused: bool,
    clock_lead_ms: u64,
}
fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}
fn strings(v: Vec<String>) -> ModelRc<slint::SharedString> {
    model(v.into_iter().map(Into::into).collect())
}
fn view(f: presentation::Function) -> FunctionView {
    FunctionView {
        title: f.title.into(),
        detail: f.detail.into(),
        icon: f.icon,
        code: f.code as i32,
    }
}
fn position(index: i32) -> String {
    match index {
        0 => "Núm trái · Nhấn".into(),
        1 => "Núm phải · Nhấn".into(),
        2..=9 => format!("Phím {}", index - 1),
        10 => "Núm trái · Xoay trái".into(),
        11 => "Núm trái · Xoay phải".into(),
        12 => "Núm phải · Xoay trái".into(),
        _ => "Núm phải · Xoay phải".into(),
    }
}
fn status(u: &MainWindow, text: impl Into<String>, error: bool) {
    u.set_status(text.into().into());
    u.set_status_error(error);
}
fn start(u: &MainWindow, tx: &mpsc::Sender<Job>, job: Job, text: &str) -> Result<()> {
    ensure!(!u.get_busy(), "Đang xử lý thao tác trước");
    u.set_busy(true);
    status(u, text, false);
    if tx.send(job).is_err() {
        u.set_busy(false);
        bail!("Không thể giao tiếp bàn phím; mở lại Coban-mod");
    }
    Ok(())
}
fn ready(u: &MainWindow) -> Result<()> {
    ensure!(
        u.get_connected() && !u.get_stale(),
        "Kết nối và đọc lại bàn phím trước khi chỉnh"
    );
    ensure!(!u.get_busy(), "Đang lưu thay đổi trước");
    Ok(())
}
fn named_function(code: u16, d: &Snapshot, host: &HostConfig) -> presentation::Function {
    name_function(presentation::describe(code, Some(d)), host)
}
fn name_function(mut f: presentation::Function, host: &HostConfig) -> presentation::Function {
    let code = f.code;
    if (104..=115).contains(&code)
        && host
            .profiles
            .iter()
            .any(|p| p.bindings.iter().any(|b| b.trigger == code))
    {
        f.title = format!("Hành động {}", code - 103);
        f.detail = "Hành động máy tính · Tự động hóa".into();
        f.icon = 24;
    }
    if let Some(name) = host.display_name(code) {
        f.title = name.into();
        f.detail = if (104..=115).contains(&code) {
            format!("{} · {}", presentation::key_name(code), f.detail)
        } else {
            format!("M{} · Chuỗi thao tác", code - 0x7700)
        };
    }
    f
}
fn refresh(u: &MainWindow, s: &Rc<RefCell<State>>) {
    let st = s.borrow();
    if let Ok(d) = st.editor.current() {
        u.set_has_host_bindings(
            d.keymap
                .iter()
                .chain(d.encoders.iter().flatten())
                .any(|c| (104..=115).contains(c)),
        );
        let l = (u.get_layer().max(0) as usize).min(d.layers as usize - 1);
        let key = u.get_selected_key().clamp(0, 13) as usize;
        u.set_layer(l as i32);
        u.set_layers(strings(
            (0..d.layers)
                .map(|n| {
                    format!(
                        "{}  {}",
                        n + 1,
                        match n {
                            0 => "Hằng ngày",
                            1 => "Soạn thảo",
                            2 => "Sáng tạo",
                            3 => "Tự chọn",
                            _ => "Lớp phím",
                        }
                    )
                })
                .collect(),
        ));
        u.set_bindings(model(
            (0..14)
                .map(|i| view(named_function(d.binding(l, i), d, &st.host)))
                .collect(),
        ));
        u.set_selected_function(view(named_function(d.binding(l, key), d, &st.host)));
        let selected_code = d.binding(l, key);
        u.set_can_name_key(matches!(selected_code, 104..=115 | 0x7700..=0x777f));
        if !u.get_key_name_editing() {
            u.set_key_display_name(st.host.display_name(selected_code).unwrap_or("").into());
        }
        u.set_assignment_target(format!("{} · Lớp {}", position(key as i32), l + 1).into());
        u.set_macro_list(strings(
            (0..d.macro_count)
                .map(|i| {
                    format!(
                        "M{i} · {}",
                        named_function(0x7700 + i as u16, d, &st.host).title
                    )
                })
                .collect(),
        ));
        let mode = d
            .settings
            .iter()
            .find(|v| v.key == "screen")
            .map_or(0, |v| v.value[0]);
        u.set_screen_is_gif(mode == 2);
        u.set_screen_mode_index(match mode {
            5 => 4,
            0..=3 => mode as i32,
            _ => 0,
        });
        u.set_screen_title(
            match mode {
                0 => "Đồng hồ",
                1 => "Thông tin máy",
                2 => "Ảnh / GIF",
                3 => "Lớp phím",
                5 => "Pomodoro",
                _ => "Màn hình",
            }
            .into(),
        );
        u.set_clock_text(chrono::Local::now().format("%H:%M").to_string().into());
    }
    u.set_host_triggers(strings(
        (104..=115)
            .map(|code| {
                format!(
                    "{} · {}",
                    presentation::key_name(code),
                    st.host
                        .display_name(code)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Hành động {}", code - 103))
                )
            })
            .collect(),
    ));
    if !u.get_host_trigger_name_editing() {
        let trigger = st.host_view.map_or(u.get_host_trigger(), |(_, t)| t);
        u.set_host_trigger_name(
            st.host
                .display_name(104 + trigger as u16)
                .unwrap_or("")
                .into(),
        );
    }
    if !u.get_macro_name_editing() {
        u.set_macro_display_name(
            st.host
                .display_name(0x7700 + st.macro_view as u16)
                .unwrap_or("")
                .into(),
        );
    }
    u.set_can_undo(!st.undo.is_empty() && u.get_connected() && !u.get_stale());
    u.set_history(strings(st.history.clone()));
}
fn refresh_catalog(u: &MainWindow, s: &Rc<RefCell<State>>) {
    let st = s.borrow();
    let d = st.editor.current().ok();
    let mut entries = presentation::catalog_named(
        u.get_search().as_str(),
        u.get_category(),
        u.get_key_group(),
        d,
        |f| name_function(f, &st.host),
    );
    if u.get_category() == 3 && u.get_search().is_empty() {
        for n in 0..12 {
            let code = 104 + n;
            entries.insert(
                n as usize,
                name_function(
                    presentation::Function {
                        code,
                        title: format!("Hành động {}", n + 1),
                        detail: "Trên máy tính · Coban-mod cần chạy".into(),
                        icon: 24,
                    },
                    &st.host,
                ),
            );
        }
    }
    u.set_functions(model(entries.into_iter().map(view).collect()));
}
fn record(s: &mut State, text: &str) {
    s.history.insert(
        0,
        format!("{} · {text}", chrono::Local::now().format("%H:%M:%S")),
    );
    s.history.truncate(30);
}
fn push_undo(s: &mut State, undo: Undo) {
    s.undo.push(undo);
    if s.undo.len() > 20 {
        s.undo.remove(0);
    }
}
fn write_request(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    tx: &mpsc::Sender<Job>,
    change: Option<Change>,
) -> Result<()> {
    if let Some(change) = change
        && let Err(e) = start(
            u,
            tx,
            Job::Write(Box::new(change)),
            "Đang sao lưu và lưu lên bàn phím…",
        )
    {
        s.borrow_mut().editor.fail();
        u.set_connected(false);
        u.set_stale(true);
        return Err(e);
    }
    Ok(())
}
fn change(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    tx: &mpsc::Sender<Job>,
    description: String,
    edit: impl FnOnce(&mut Snapshot) -> Result<()>,
) -> Result<()> {
    ready(u)?;
    let mut after = s.borrow().editor.current()?.clone();
    edit(&mut after)?;
    let request = s.borrow_mut().editor.begin(after, description)?;
    write_request(u, s, tx, request)
}
fn assign(u: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>, code: u16) -> Result<()> {
    let title = presentation::describe(code, s.borrow().editor.current().ok()).title;
    change(
        u,
        s,
        tx,
        format!(
            "Đã gán {title} cho {}",
            position(u.get_selected_key()).to_lowercase()
        ),
        |d| {
            mk::keycodes::validate_for_device(code, d.layers, d.macro_count)?;
            d.set_binding(u.get_layer() as usize, u.get_selected_key() as usize, code)
        },
    )
}
fn show_macro(u: &MainWindow, s: &Rc<RefCell<State>>, index: usize) {
    let snapshot = s.borrow().editor.current().ok().cloned();
    let Some(d) = snapshot else { return };
    if index >= d.macro_count as usize {
        return;
    }
    s.borrow_mut().rendering = true;
    s.borrow_mut().macro_view = index;
    u.set_macro_index(index as i32);
    u.set_macro_name(d.macro_names[index].clone().into());
    u.set_macro_display_name(
        s.borrow()
            .host
            .display_name(0x7700 + index as u16)
            .unwrap_or("")
            .into(),
    );
    let decoded = macros::split(&d.macro_buffer, d.macro_count as usize)
        .and_then(|items| macro_steps::decode(&items[index]));
    match decoded {
        Ok(steps) => {
            u.set_macro_editable(true);
            s.borrow_mut().steps = steps;
        }
        Err(e) => {
            u.set_macro_editable(false);
            s.borrow_mut().steps.clear();
            status(u, format!("Macro này được giữ nguyên: {e:#}"), true);
        }
    }
    let steps = s.borrow().steps.clone();
    u.set_steps(model(
        steps
            .iter()
            .map(|step| StepView {
                title: match step.kind {
                    0 => "Gõ văn bản",
                    1 => "Nhấn phím / tổ hợp",
                    2 => "Chờ",
                    3 => "Giữ phím",
                    _ => "Nhả phím",
                }
                .into(),
                detail: step
                    .text
                    .replace('\n', " ↵ ")
                    .chars()
                    .take(45)
                    .collect::<String>()
                    .into(),
                icon: match step.kind {
                    0 => 31,
                    2 => 32,
                    _ => 34,
                },
            })
            .collect(),
    ));
    let selected = u.get_step_index().min(steps.len() as i32 - 1);
    u.set_step_index(selected);
    if selected >= 0 {
        u.set_step_kind(steps[selected as usize].kind);
        u.set_step_text(steps[selected as usize].text.clone().into());
    } else {
        u.set_step_text("".into());
    }
    let used = macros::split(&d.macro_buffer, d.macro_count as usize)
        .map(|items| items.iter().map(|x| x.len() + 1).sum::<usize>())
        .unwrap_or(d.macro_buffer.len());
    u.set_macro_info(
        format!(
            "Bộ nhớ dùng chung: {used} / {} byte. Các macro khác được giữ nguyên.",
            d.macro_buffer.len()
        )
        .into(),
    );
    s.borrow_mut().rendering = false;
}
fn commit_steps(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    tx: &mpsc::Sender<Job>,
    steps: Vec<Step>,
) -> Result<()> {
    let encoded = macro_steps::encode(&steps)?;
    let index = s.borrow().macro_view;
    let name = u.get_macro_name().to_string();
    ensure!(
        name.len() <= 6 && name.bytes().all(|b| b.is_ascii_graphic() || b == b' '),
        "Tên trên bàn phím tối đa 6 ký tự không dấu"
    );
    change(
        u,
        s,
        tx,
        format!("Đã cập nhật các bước của macro {}", index + 1),
        |d| {
            d.macro_buffer =
                macros::replace(&d.macro_buffer, d.macro_count as usize, index, &encoded)?;
            d.macro_names[index] = name;
            Ok(())
        },
    )
}
fn edited_steps(u: &MainWindow, s: &Rc<RefCell<State>>) -> Result<Vec<Step>> {
    let mut steps = s.borrow().steps.clone();
    if u.get_step_index() >= 0 && (u.get_step_index() as usize) < steps.len() {
        macro_steps::set_text(
            &mut steps,
            u.get_step_index() as usize,
            u.get_step_text().to_string(),
        )?;
    }
    Ok(steps)
}
fn finish_edits(u: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) -> Result<()> {
    if u.get_busy() || s.borrow().rendering {
        return Ok(());
    }
    if u.get_page() != 5 && commit_display_names(u, s)? {
        return Ok(());
    }
    match u.get_page() {
        1 if u.get_connected() && u.get_macro_editable() => {
            commit_steps(u, s, tx, edited_steps(u, s)?)
        }
        5 => {
            commit_host(u, s)?;
            if !u.get_busy() {
                commit_display_names(u, s)?;
            }
            Ok(())
        }
        3 if u.get_screen_is_gif()
            && u.get_screen_source() == 1
            && !u.get_screen_more_settings() =>
        {
            commit_screen(u, s, tx)?;
            Ok(())
        }
        0 if u.get_key_panel() > 0 => {
            commit_screen(u, s, tx)?;
            Ok(())
        }
        0 if u.get_connected() && u.get_advanced() && !u.get_chord_input().trim().is_empty() => {
            let code = macro_steps::parse_shortcut(u.get_chord_input().as_str())?;
            assign(u, s, tx, code)?;
            u.set_chord_input("".into());
            Ok(())
        }
        _ => Ok(()),
    }
}
fn make_field(key: &str, index: usize, label: &str, value: u8, max: u8) -> SettingField {
    let mut options = mk::protocol::field_options(key, index);
    if !options.is_empty() && !options.iter().any(|(v, _)| *v == value) {
        options.push((value, format!("Giá trị hiện tại: {value}")));
    }
    let current = options.iter().position(|(v, _)| *v == value).unwrap_or(0);
    let kind = if max == 1 {
        2
    } else if !options.is_empty() {
        3
    } else if matches!(
        key,
        "brightness" | "rgb_brightness" | "rgb_speed" | "rgb_color" | "theme"
    ) {
        1
    } else {
        0
    };
    let label = label
        .replace(" (0/1)", "")
        .replace(" (0–12; xem danh sách bên dưới)", "");
    SettingField {
        label: label.into(),
        value: value as i32,
        maximum: max.max(value) as i32,
        options: strings(options.iter().map(|(_, s)| s.clone()).collect()),
        option_values: model(options.iter().map(|(n, _)| *n as i32).collect()),
        option_index: current as i32,
        kind,
    }
}
fn show_settings(u: &MainWindow, s: &Rc<RefCell<State>>, index: i32) {
    let group = match u.get_page() {
        2 => "Đèn phím",
        4 => "Âm thanh",
        _ => "Màn hình",
    };
    let specs = setting_specs();
    let mut st = s.borrow_mut();
    let Ok(d) = st.editor.current().cloned() else {
        return;
    };
    let keys: Vec<_> = specs
        .iter()
        .filter(|x| x.group == group && d.settings.iter().any(|v| v.key == x.key))
        .map(|x| x.key.to_string())
        .collect();
    u.set_settings_list(strings(
        keys.iter()
            .filter_map(|key| specs.iter().find(|x| x.key == key).map(|x| x.title.into()))
            .collect(),
    ));
    let i = (index.max(0) as usize).min(keys.len().saturating_sub(1));
    u.set_setting_index(i as i32);
    u.set_stats_selected(keys.get(i).is_some_and(|k| k == "stats"));
    let fields = keys
        .get(i)
        .and_then(|key| {
            specs.iter().find(|x| x.key == key).and_then(|spec| {
                d.settings
                    .iter()
                    .find(|x| x.key.as_str() == key.as_str())
                    .map(|v| {
                        spec.fields
                            .iter()
                            .zip(&v.value)
                            .enumerate()
                            .map(|(j, ((label, max), value))| {
                                make_field(spec.key, j, label, *value, *max)
                            })
                            .collect()
                    })
            })
        })
        .unwrap_or_default();
    st.setting_keys = keys;
    u.set_setting_fields(model(fields));
}
fn preview_image(png: &[u8]) -> Result<slint::Image> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        img.as_raw(),
        img.width(),
        img.height(),
    );
    Ok(slint::Image::from_rgba8(buffer))
}
fn show_screen(u: &MainWindow, s: &Rc<RefCell<State>>, png: Option<&[u8]>) -> Result<()> {
    let st = s.borrow();
    let cfg = &st.screen_layout;
    let tile = cfg.tile(st.screen_tile)?;
    if let Some(png) = png {
        u.set_layout_preview(preview_image(png)?);
    }
    u.set_layout_loaded(st.screen_loaded);
    u.set_layout_tile(st.screen_tile as i32);
    u.set_layout_cell_label(screen_layout::label(st.screen_tile).into());
    u.set_layout_bg_kind(if cfg.use_background_image { 1 } else { 0 });
    u.set_layout_bg_color(screen_layout::color_text(cfg.background).into());
    u.set_layout_ink_color(screen_layout::color_text(cfg.ink).into());
    u.set_layout_use_common_ink(tile.ink.is_none());
    u.set_layout_tint_image(tile.tint_image);
    u.set_layout_cell_ink(screen_layout::color_text(tile.ink.unwrap_or(cfg.ink)).into());
    u.set_layout_icon_size(match tile.icon_size {
        screen_layout::IconSize::Large => 0,
        screen_layout::IconSize::Medium => 1,
        screen_layout::IconSize::Small => 2,
    });
    u.set_layout_tile_background(cfg.tile_background);
    u.set_layout_bg_name(
        cfg.background_image
            .as_ref()
            .map_or("", |a| a.name.as_str())
            .into(),
    );
    u.set_layout_kind(match tile.kind {
        Content::Text => 0,
        Content::Image => 1,
        Content::Symbol => 2,
        Content::Clock => 3,
        Content::Empty => 4,
    });
    u.set_layout_text(tile.text.clone().into());
    u.set_layout_text_size(tile.text_size as i32);
    u.set_layout_image_name(tile.image.as_ref().map_or("", |a| a.name.as_str()).into());
    u.set_layout_symbol_name(tile.symbol.clone().into());
    u.set_layout_symbol_weight(tile.symbol_weight as i32);
    u.set_layout_symbol_label(
        mk::screen_symbols::CURATED
            .iter()
            .find(|(_, n)| *n == tile.symbol)
            .map_or(tile.symbol.clone(), |(l, n)| format!("{l} · {n}"))
            .into(),
    );
    let clock = tile.clock;
    u.set_layout_clock_face(clock.face as i32);
    u.set_layout_clock_layout(clock.layout as i32);
    u.set_layout_clock_hour24(clock.hour24);
    u.set_layout_clock_ampm(clock.ampm);
    u.set_layout_clock_date(clock.date as i32);
    drop(st);
    show_symbol_choices(u)
}
fn icon_weight(index: i32) -> Result<IconWeight> {
    Ok(match index {
        0 => IconWeight::Regular,
        1 => IconWeight::Bold,
        2 => IconWeight::Fill,
        _ => bail!("Nét biểu tượng không hợp lệ"),
    })
}
/// Phosphor picker grid for the current search, drawn in the selected weight.
fn show_symbol_choices(u: &MainWindow) -> Result<()> {
    let weight = icon_weight(u.get_layout_symbol_weight())?;
    let choices = mk::screen_symbols::search(&u.get_layout_symbol_query(), 60)
        .into_iter()
        .map(|(label, name)| {
            let art = mk::screen_symbols::mask(name, weight, 48)?;
            Ok(IconChoice {
                name: name.into(),
                label: label.into(),
                art: slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                    art.as_raw(),
                    art.width(),
                    art.height(),
                )),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    u.set_layout_symbol_choices(model(choices));
    Ok(())
}
fn screen_form(u: &MainWindow, s: &Rc<RefCell<State>>) -> Result<Layout> {
    let st = s.borrow();
    ensure!(st.screen_loaded, "Đang tải bố cục màn hình");
    let mut cfg = st.screen_layout.clone();
    cfg.background = screen_layout::parse_color(&u.get_layout_bg_color())?;
    cfg.ink = screen_layout::parse_color(&u.get_layout_ink_color())?;
    cfg.use_background_image = u.get_layout_bg_kind() == 1;
    cfg.tile_background = u.get_layout_tile_background();
    let tile = cfg.tile_mut(st.screen_tile)?;
    tile.kind = match u.get_layout_kind() {
        1 => Content::Image,
        2 => Content::Symbol,
        3 => Content::Clock,
        4 => Content::Empty,
        _ => Content::Text,
    };
    tile.text = u.get_layout_text().to_string();
    tile.text_size = u.get_layout_text_size() as u8;
    tile.tint_image = u.get_layout_tint_image();
    tile.ink = if u.get_layout_use_common_ink() {
        None
    } else {
        Some(screen_layout::parse_color(&u.get_layout_cell_ink())?)
    };
    tile.icon_size = match u.get_layout_icon_size() {
        0 => screen_layout::IconSize::Large,
        1 => screen_layout::IconSize::Medium,
        2 => screen_layout::IconSize::Small,
        _ => anyhow::bail!("Cỡ icon không hợp lệ"),
    };
    if tile.kind == Content::Symbol {
        tile.symbol = u.get_layout_symbol_name().trim().to_string();
        tile.symbol_weight = icon_weight(u.get_layout_symbol_weight())?;
    }
    if tile.kind == Content::Clock {
        tile.clock = screen_layout::Clock {
            face: match u.get_layout_clock_face() {
                1 => ClockFace::Bold,
                2 => ClockFace::Segment,
                _ => ClockFace::Regular,
            },
            layout: match u.get_layout_clock_layout() {
                1 => ClockLayout::Time,
                2 => ClockLayout::WeekdayDate,
                3 => ClockLayout::Inline,
                _ => ClockLayout::TimeDate,
            },
            hour24: u.get_layout_clock_hour24(),
            ampm: u.get_layout_clock_ampm(),
            date: match u.get_layout_clock_date() {
                1 => DateFormat::DayMonthYear,
                2 => DateFormat::MonthDayYear,
                3 => DateFormat::YearMonthDay,
                _ => DateFormat::DayMonth,
            },
        };
    }
    cfg.validate()?;
    Ok(cfg)
}
fn preview_screen(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    tx: &mpsc::Sender<Job>,
    cfg: Layout,
    force: bool,
) -> Result<bool> {
    if !force && cfg == s.borrow().screen_layout {
        return Ok(false);
    }
    ready(u)?;
    let revision = s.borrow().screen_revision;
    start(
        u,
        tx,
        Job::RenderScreen {
            layer: s.borrow().screen_layer,
            expected_revision: revision,
            layout: cfg.clone(),
        },
        "Đang tạo ảnh xem trước…",
    )?;
    let mut st = s.borrow_mut();
    st.screen_layout = cfg;
    st.screen_rendering = true;
    st.apply_after_preview = true;
    st.screen_preview_valid = false;
    u.set_layout_save_failed(false);
    u.set_layout_preview_pending(true);
    u.set_layout_status("Đang tạo và tự gửi ảnh lên bàn phím…".into());
    Ok(true)
}
fn request_layer(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    tx: &mpsc::Sender<Job>,
    layer: u8,
) -> Result<()> {
    if u.get_busy() {
        s.borrow_mut().pending_screen_layer = Some(layer);
        return Ok(());
    }
    start(
        u,
        tx,
        Job::LoadScreen { layer },
        "Đang tải ảnh và chức năng của lớp…",
    )?;
    s.borrow_mut().screen_loading = true;
    Ok(())
}
fn commit_screen(u: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) -> Result<bool> {
    if u.get_busy() || !s.borrow().screen_loaded {
        return Ok(false);
    }
    let result = screen_form(u, s).and_then(|cfg| preview_screen(u, s, tx, cfg, false));
    if let Err(error) = &result {
        u.set_layout_status(format!("{error:#}").into());
    }
    result
}
fn apply_screen(u: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) -> Result<()> {
    ready(u)?;
    let before = s.borrow().editor.current()?.clone();
    let revision = s.borrow().screen_revision;
    start(
        u,
        tx,
        Job::ApplyScreen {
            layer: s.borrow().screen_layer,
            revision,
            before: Box::new(before),
        },
        "Đang tạo, sao lưu và gửi ảnh lên màn hình…",
    )?;
    let mut st = s.borrow_mut();
    st.screen_applying = true;
    // The agent renders clock tiles as it starts this job.
    st.clock_sending = Some(minute(chrono::Local::now().naive_local()));
    drop(st);
    u.set_layout_save_failed(false);
    u.set_layout_status("Đang gửi ảnh; chờ bàn phím xác nhận lưu.".into());
    Ok(())
}
fn minute(at: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    use chrono::Timelike;
    at.with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(at)
}
/// Once a minute, redraws clock tiles and resends the GIF without blocking edits.
/// Starts early by the last upload time so the new minute appears on time.
fn clock_tick(u: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) {
    let mut st = s.borrow_mut();
    if st.layer_monitoring
        || !st.screen_loaded
        || !st.screen_layout.has_clock()
        || st.clock_refreshing.is_some()
        || st.clock_paused
        || st.screen_rendering
        || st.screen_applying
        || st.apply_after_preview
        || !st.screen_preview_valid
        || u.get_busy()
        || !u.get_connected()
        || u.get_stale()
    {
        return;
    }
    let Some(screen) = st
        .editor
        .current()
        .ok()
        .and_then(|d| d.settings.iter().find(|v| v.key == "screen"))
        .map(|v| v.value.clone())
        .filter(|v| v.first() == Some(&2))
    else {
        return;
    };
    let lead = chrono::Duration::milliseconds(st.clock_lead_ms.min(8_000) as i64);
    let target = minute(chrono::Local::now().naive_local() + lead);
    if st.clock_shown == Some(target) {
        return;
    }
    let job = Job::RefreshClock {
        layer: st.screen_layer,
        revision: st.screen_revision,
        at: target,
        screen,
    };
    if tx.send(job).is_ok() {
        st.clock_refreshing = Some(target);
    }
}
fn show_host(u: &MainWindow, s: &Rc<RefCell<State>>, index: i32) {
    s.borrow_mut().rendering = true;
    s.borrow_mut().host_view = Some((index, u.get_host_trigger()));
    let st = s.borrow();
    u.set_host_trigger_name(
        st.host
            .display_name(104 + u.get_host_trigger() as u16)
            .unwrap_or("")
            .into(),
    );
    u.set_host_profile_index(index);
    if let Some(p) = st
        .host
        .profiles
        .get(index.max(0) as usize)
        .filter(|_| index >= 0)
    {
        u.set_host_name(p.name.clone().into());
        u.set_host_app(p.application.clone().into());
        match p
            .bindings
            .iter()
            .find(|b| b.trigger == 104 + u.get_host_trigger() as u16)
            .map(|b| b.action.clone().upgraded())
        {
            Some(HostAction::Text(t)) => {
                u.set_host_kind(0);
                u.set_host_text(t.clone().into());
            }
            Some(HostAction::Web(t)) => {
                u.set_host_kind(1);
                u.set_host_text(t.clone().into());
            }
            Some(HostAction::Application(t)) => {
                u.set_host_kind(2);
                u.set_host_text(t.clone().into());
            }
            Some(HostAction::File(t)) => {
                u.set_host_kind(3);
                u.set_host_text(t.into());
            }
            Some(HostAction::Macro(t)) => {
                u.set_host_kind(4);
                u.set_host_text(t.into());
            }
            Some(HostAction::Open(_)) => unreachable!("legacy action was upgraded"),
            None => {
                u.set_host_kind(1);
                u.set_host_text("".into());
            }
        }
    } else {
        u.set_host_name("Mặc định".into());
        u.set_host_app("".into());
        u.set_host_kind(1);
        u.set_host_text("".into());
    }
    drop(st);
    s.borrow_mut().rendering = false;
}
fn persist_host(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    cfg: HostConfig,
    description: &str,
    undo: bool,
) -> Result<()> {
    cfg.validate()?;
    for profile in &cfg.profiles {
        for binding in &profile.bindings {
            if let HostAction::Macro(text) = &binding.action {
                host::validate_macro(text)?;
            }
        }
    }
    let data = serde_json::to_vec_pretty(&cfg)?;
    if data == serde_json::to_vec_pretty(&s.borrow().host)? {
        return Ok(());
    }
    let mut st = s.borrow_mut();
    ensure!(st.pending_host.is_none(), "Đang lưu hành động trước");
    let sender = st.sender.as_ref().context("Chưa kết nối agent")?;
    sender.send(Job::UpdateProfiles {
        expected_revision: st.host_revision,
        config: cfg.clone(),
    })?;
    let previous = st.host.clone();
    st.pending_host = Some((previous, description.into(), undo));
    st.host = cfg;
    u.set_busy(true);
    u.set_host_profiles(strings(
        st.host.profiles.iter().map(|p| p.name.clone()).collect(),
    ));
    drop(st);
    refresh(u, s);
    refresh_catalog(u, s);
    status(u, "Đang lưu cấu hình Coban-mod qua Agent…", false);
    Ok(())
}
fn commit_display_names(u: &MainWindow, s: &Rc<RefCell<State>>) -> Result<bool> {
    if s.borrow().rendering || u.get_busy() {
        return Ok(false);
    }
    let (code, name) = match u.get_page() {
        0 if u.get_connected() && u.get_can_name_key() => (
            u.get_selected_function().code as u16,
            u.get_key_display_name(),
        ),
        1 if u.get_connected() => (
            0x7700 + s.borrow().macro_view as u16,
            u.get_macro_display_name(),
        ),
        5 => {
            let trigger = s
                .borrow()
                .host_view
                .map_or(u.get_host_trigger(), |(_, t)| t);
            (104 + trigger as u16, u.get_host_trigger_name())
        }
        _ => return Ok(false),
    };
    let mut cfg = s.borrow().host.clone();
    if cfg.display_name(code).unwrap_or("") == name.trim() {
        return Ok(false);
    }
    cfg.set_display_name(code, &name)?;
    persist_host(u, s, cfg, "Đã lưu tên riêng trong Coban-mod", false)?;
    Ok(true)
}
fn commit_host(u: &MainWindow, s: &Rc<RefCell<State>>) -> Result<()> {
    if s.borrow().rendering || u.get_page() != 5 || u.get_busy() {
        return Ok(());
    }
    let name = u.get_host_name().trim().to_string();
    ensure!(!name.is_empty(), "Nhập tên bộ hành động");
    let text = u.get_host_text().to_string();
    let (index, trigger_index) = s
        .borrow()
        .host_view
        .unwrap_or((u.get_host_profile_index(), u.get_host_trigger()));
    if index < 0 && text.is_empty() {
        return Ok(());
    }
    // An unconfigured slot or a newly selected type has no completed target yet.
    // Keep the saved action until its replacement is valid, while allowing navigation.
    if (1..=3).contains(&u.get_host_kind()) && text.trim().is_empty() {
        return Ok(());
    }
    let action = match u.get_host_kind() {
        1 => {
            host::normalize_web_target(&text)?;
            HostAction::Web(text.trim().into())
        }
        2 => {
            host::validate_application_target(&text)?;
            HostAction::Application(text.trim().into())
        }
        3 => {
            ensure!(!text.trim().is_empty(), "Chọn tệp cần mở");
            HostAction::File(text.trim().into())
        }
        4 => {
            host::validate_macro(&text)?;
            HostAction::Macro(text)
        }
        _ => HostAction::Text(text),
    };
    let mut cfg = s.borrow().host.clone();
    cfg.set_display_name(104 + trigger_index as u16, &u.get_host_trigger_name())?;
    let index = if index >= 0 && (index as usize) < cfg.profiles.len() {
        index as usize
    } else {
        cfg.profiles.push(HostProfile {
            name: name.clone(),
            application: String::new(),
            bindings: Vec::new(),
        });
        cfg.profiles.len() - 1
    };
    let p = &mut cfg.profiles[index];
    p.name = name;
    p.application = u.get_host_app().to_string();
    let trigger = 104 + trigger_index as u16;
    if let Some(binding) = p.bindings.iter_mut().find(|b| b.trigger == trigger) {
        binding.action = action;
    } else {
        p.bindings.push(HostBinding { trigger, action });
    }
    persist_host(u, s, cfg, "Đã lưu hành động trên máy tính", false)?;
    u.set_host_profile_index(index as i32);
    Ok(())
}
fn host_form_dirty(u: &MainWindow, s: &Rc<RefCell<State>>) -> bool {
    if u.get_page() != 5 {
        return false;
    }
    let st = s.borrow();
    let (index, trigger) = st.host_view.unwrap_or((-1, 0));
    let Some(profile) = st.host.profiles.get(index.max(0) as usize) else {
        return !u.get_host_text().is_empty();
    };
    if u.get_host_name().as_str() != profile.name
        || u.get_host_app().as_str() != profile.application
        || u.get_host_trigger_name().trim()
            != st.host.display_name(104 + trigger as u16).unwrap_or("")
    {
        return true;
    }
    let saved = profile
        .bindings
        .iter()
        .find(|b| b.trigger == 104 + trigger as u16)
        .map(|b| match &b.action {
            HostAction::Text(text) => (0, text.as_str()),
            HostAction::Web(text) => (1, text.as_str()),
            HostAction::Application(text) => (2, text.as_str()),
            HostAction::File(text) => (3, text.as_str()),
            HostAction::Macro(text) => (4, text.as_str()),
            HostAction::Open(_) => unreachable!("legacy upgraded"),
        });
    match saved {
        Some((kind, text)) => kind != u.get_host_kind() || text != u.get_host_text().as_str(),
        None => !u.get_host_text().is_empty(),
    }
}
fn handle_event(
    u: &MainWindow,
    s: &Rc<RefCell<State>>,
    sender: &mpsc::Sender<Job>,
    event: Event,
) -> Result<()> {
    let terminal = !matches!(
        &event,
        Event::Devices(_, false)
            | Event::HostStatus(_)
            | Event::Profiles { .. }
            | Event::LoginStatus(_)
            | Event::ClockRefreshed { .. }
    );
    if terminal {
        u.set_busy(false);
    }
    match event {
        Event::LayerConfirmed(result) => {
            s.borrow_mut().editor.replace(result.snapshot)?;
            s.borrow_mut().undo.clear();
            if let Some(path) = result.backup {
                u.set_backup_info(format!("Bản sao trước đồng bộ: {}", path.display()).into());
            }
            u.set_follow_layer(true);
            s.borrow_mut().inspection_origin = None;
            refresh(u, s);
            sender.send(Job::Status)?;
            status(u, "Đã cập nhật tự đổi ảnh theo lớp bàn phím", false);
        }
        Event::ScreenStored { layer } => {
            let mut st = s.borrow_mut();
            st.screen_applying = false;
            st.clock_sending = None;
            st.clock_paused = false;
            u.set_layout_save_failed(false);
            u.set_layout_status(
                format!(
                    "Đã lưu ảnh lớp {} trên máy; sẽ gửi khi lớp này hoạt động",
                    layer + 1
                )
                .into(),
            );
            status(u, format!("Đã lưu ảnh lớp {}", layer + 1), false);
            let next = st.after_device_write.take();
            drop(st);
            if let Some(job) = next {
                start(u, sender, job, "Đang dừng dịch vụ…")?;
            }
        }
        Event::Devices(devices, interactive) => {
            if !interactive && u.get_busy() {
                return Ok(());
            }
            let count = devices.len();
            u.set_devices(strings(
                devices
                    .iter()
                    .map(|d| {
                        format!(
                            "{} · {}",
                            d.identity.product,
                            d.identity.serial.as_deref().unwrap_or("USB")
                        )
                    })
                    .collect(),
            ));
            let mut st = s.borrow_mut();
            if u.get_connected()
                && st
                    .active_path
                    .as_ref()
                    .is_some_and(|path| !devices.iter().any(|d| &d.path == path))
            {
                st.editor.fail();
                u.set_connected(false);
                u.set_stale(true);
                u.set_can_undo(false);
                status(
                    u,
                    "Bàn phím đã ngắt kết nối. Kết nối lại để chỉnh tiếp.",
                    true,
                );
            }
            st.devices = devices;
            let automatic = st
                .active_path
                .as_ref()
                .and_then(|path| st.devices.iter().position(|d| &d.path == path))
                .or_else(|| (count == 1).then_some(0));
            if let Some(index) = automatic.filter(|_| !st.ever_connected && !u.get_connected()) {
                let d = st.devices[index].clone();
                u.set_device_index(index as i32);
                st.active_path = Some(d.path.clone());
                drop(st);
                start(u, sender, Job::Connect(d), "Đang đọc Bao-keyboard…")?;
            } else if count == 0 && !st.ever_connected {
                status(
                    u,
                    "Chưa tìm thấy Bao-keyboard. Cắm cáp USB rồi chọn Tìm bàn phím.",
                    false,
                );
            }
        }
        Event::Snapshot(d) => {
            ensure!(
                !d.simulated || cfg!(feature = "internal-ui-qa"),
                "Không thể dùng dữ liệu minh họa để điều khiển thiết bị"
            );
            s.borrow_mut().editor.replace(d.clone())?;
            s.borrow_mut().undo.clear();
            s.borrow_mut().ever_connected = true;
            s.borrow_mut().auto_actions = false;
            u.set_connected(true);
            u.set_stale(false);
            u.set_device_detail(
                format!(
                    "{} · {} lớp phím · {} macro\nĐọc lúc {} · Giữ nguyên firmware gốc",
                    d.device.product, d.layers, d.macro_count, d.captured_at
                )
                .into(),
            );
            u.set_warnings(d.warnings.join("\n").into());
            refresh(u, s);
            refresh_catalog(u, s);
            show_macro(u, s, u.get_macro_index().max(0) as usize);
            show_settings(u, s, 0);
            status(
                u,
                "Đã kết nối. Chọn chức năng để đổi ngay trên bàn phím.",
                false,
            );
            if !s.borrow().screen_loaded && !s.borrow().screen_loading {
                s.borrow_mut().screen_loading = true;
                start(
                    u,
                    sender,
                    Job::LoadScreen { layer: 0 },
                    "Đang tải bố cục màn hình…",
                )?;
            }
        }
        Event::ScreenDraft {
            layer,
            revision,
            layout,
            png,
            preview_png,
        }
        | Event::ScreenPreview {
            layer,
            revision,
            layout,
            png,
            preview_png,
        } => {
            let mut st = s.borrow_mut();
            st.screen_layer = layer;
            u.set_layer(layer as i32);
            st.screen_layout = layout;
            st.screen_revision = revision;
            st.screen_loaded = true;
            st.screen_loading = false;
            st.screen_rendering = false;
            st.screen_preview_valid = true;
            u.set_layout_preview_pending(false);
            let apply = std::mem::take(&mut st.apply_after_preview);
            drop(st);
            refresh(u, s);
            refresh_catalog(u, s);
            show_screen(u, s, Some(&png))?;
            u.set_layout_sharp_preview(preview_image(if preview_png.is_empty() {
                &png
            } else {
                &preview_png
            })?);
            u.set_layout_status("Ảnh lưu riêng theo lớp; gửi lên LCD khi lớp đó hoạt động.".into());
            status(
                u,
                if u.get_connected() {
                    "Đã kết nối. Chọn chức năng để đổi ngay trên bàn phím."
                } else {
                    "Đã tải bố cục tùy chỉnh trên máy."
                },
                false,
            );
            // Loading/reopening a layout never writes hardware. Only a completed edit does.
            if apply && let Err(error) = apply_screen(u, s, sender) {
                u.set_layout_save_failed(true);
                u.set_layout_status(format!("{error:#}").into());
                return Err(error);
            }
        }
        Event::ScreenAsset(asset) => {
            let background = s
                .borrow_mut()
                .screen_asset_target
                .take()
                .context("Không có ô ảnh đang chọn")?;
            let mut cfg = s.borrow().screen_layout.clone();
            if background {
                cfg.background_image = Some(asset);
                cfg.use_background_image = true;
            } else {
                let i = s.borrow().screen_tile;
                let tile = cfg.tile_mut(i)?;
                tile.image = Some(asset);
                tile.kind = Content::Image;
            }
            preview_screen(u, s, sender, cfg, true)?;
        }
        Event::ClockRefreshed {
            layer,
            revision,
            png,
            preview_png,
            millis,
            error,
        } => {
            let mut st = s.borrow_mut();
            let sent = st.clock_refreshing.take();
            if let Some(error) = error {
                // Pause ticks until the next successful upload (edit or Thử gửi lại).
                st.clock_paused = true;
                drop(st);
                u.set_layout_save_failed(true);
                u.set_layout_status(format!("Chưa cập nhật đồng hồ: {error}").into());
                return Ok(());
            }
            st.clock_shown = sent;
            st.clock_lead_ms = millis + 300;
            let current =
                layer == st.screen_layer && revision == st.screen_revision && !st.screen_rendering;
            drop(st);
            if current {
                u.set_layout_preview(preview_image(&png)?);
                u.set_layout_sharp_preview(preview_image(&preview_png)?);
            }
        }
        Event::ScreenApplied { result, output: _ } => {
            let mut st = s.borrow_mut();
            st.screen_applying = false;
            st.clock_paused = false;
            st.clock_shown = st.clock_sending.take();
            u.set_layout_save_failed(false);
            st.editor.replace(result.snapshot)?;
            st.undo.clear();
            record(
                &mut st,
                "Đã gửi ảnh màn hình; không thể hoàn tác ảnh cũ trong firmware",
            );
            let next = st.after_device_write.take();
            drop(st);
            if let Some(path) = result.backup {
                u.set_backup_info(format!("Bản sao cấu hình: {}", path.display()).into());
            }
            refresh(u, s);
            show_settings(u, s, u.get_setting_index());
            u.set_layout_status("Đã tự gửi ảnh · Bàn phím đã xác nhận lưu.".into());
            status(
                u,
                "Đã gửi ảnh và xác minh chế độ Ảnh/GIF. Kiểm tra hình hiển thị trên bàn phím.",
                false,
            );
            if let Some(job) = next {
                start(u, sender, job, "Đang dừng dịch vụ…")?;
            }
        }
        Event::Written(result) => {
            let mut st = s.borrow_mut();
            let description = st.editor.confirm(result.snapshot)?;
            if st.pending_undo {
                st.undo.pop();
                st.pending_undo = false;
            } else {
                push_undo(&mut st, Undo::Device);
            }
            record(&mut st, &description);
            drop(st);
            if let Some(path) = result.backup {
                u.set_backup_info(format!("Bản sao tự động: {}", path.display()).into());
            }
            refresh(u, s);
            refresh_catalog(u, s);
            show_macro(u, s, u.get_macro_index().max(0) as usize);
            show_settings(u, s, u.get_setting_index());
            status(u, format!("{description} · Đã đọc lại và xác minh"), false);
            let next = s.borrow_mut().after_device_write.take();
            if let Some(job) = next {
                start(u, sender, job, "Đang dừng dịch vụ…")?;
            } else if s.borrow().quit_after_save {
                slint::quit_event_loop()?;
            }
        }
        Event::Backup(path) => {
            u.set_backup_info(path.display().to_string().into());
            status(u, "Đã xuất bản sao cấu hình bàn phím.", false);
        }
        Event::Done(text) => {
            status(u, text, false);
            if s.borrow().stop_after_response {
                slint::quit_event_loop()?;
            }
        }
        Event::LoginStatus(login) | Event::LoginChanged(login) => {
            u.set_host_login_available(login.available);
            u.set_host_login_enabled(login.enabled);
            u.set_host_login_status(login.message.clone().into());
            if terminal {
                status(u, login.message, false);
            }
        }
        Event::HostStatus(agent) | Event::HostChanged(agent) => {
            let active = agent.layer_sync.active;
            u.set_active_layer(active.map_or(-1, |n| n as i32));
            u.set_displayed_layer(agent.layer_sync.displayed.map_or(-1, |n| n as i32));
            u.set_layer_sync_message(agent.layer_sync.message.clone().into());
            u.set_layer_sync_supported(agent.layer_sync.supported);
            u.set_layer_sync_image_safe(agent.layer_sync.image_safe);
            u.set_layer_sync_enabled(agent.layer_sync.enabled);
            let mut st = s.borrow_mut();
            st.layer_monitoring = true;
            st.poll_pending = false;
            if st
                .inspection_origin
                .is_some_and(|origin| active != Some(origin))
            {
                st.inspection_origin = None;
            }
            let inspecting = st.inspection_origin.is_some();
            if !st.agent_instance.is_empty() && st.agent_instance != agent.instance {
                st.auto_actions = false;
                st.editor.fail();
                st.undo.clear();
                st.ever_connected = false;
                u.set_connected(false);
                u.set_stale(true);
                u.set_can_undo(false);
            }
            if st.host_revision != agent.profile_revision && st.pending_host.is_none() {
                sender.send(Job::Profiles)?;
            }
            st.agent_instance = agent.instance;
            if let Some(selected) = agent.selected {
                if u.get_connected() && st.active_path.as_ref() != Some(&selected.path) {
                    st.editor.fail();
                    u.set_connected(false);
                    u.set_stale(true);
                    u.set_can_undo(false);
                }
                st.active_path = Some(selected.path);
            }
            u.set_host_enabled(agent.enabled);
            u.set_host_running(agent.enabled && agent.state == host::HostState::Ready);
            let auto_start = !agent.enabled
                && !st.auto_actions
                && st.pending_host.is_none()
                && !st.stop_after_response
                && u.get_connected()
                && !u.get_stale()
                && !u.get_busy();
            let label = match agent.state {
                host::HostState::Stopped => "Coban-mod Agent sẵn sàng · Nhận phím đang tắt",
                host::HostState::Starting => "Coban-mod Agent đang khởi động bộ nhận phím",
                host::HostState::WaitingForDevice => "Đang đợi bàn phím đã chọn kết nối lại",
                host::HostState::NeedsInputPermission => {
                    "Coban-mod Agent chưa có quyền nhận phím của macOS"
                }
                host::HostState::Ready => "Coban-mod Agent đang nhận phím từ bàn phím đã chọn",
                host::HostState::Degraded => "Bộ nhận phím đang phục hồi kết nối",
            };
            u.set_host_status(
                if agent.message.is_empty() || agent.state != host::HostState::Ready {
                    label.to_string()
                } else {
                    format!("{label}\n{}", agent.message)
                }
                .into(),
            );
            drop(st);
            let follow = u.get_follow_layer()
                && !inspecting
                && !u.get_busy()
                && active.is_some_and(|layer| layer != s.borrow().screen_layer);
            if follow && let Some(layer) = active {
                finish_edits(u, s, sender)?;
                request_layer(u, s, sender, layer)?;
            }
            if auto_start && !u.get_busy() {
                s.borrow_mut().auto_actions = true;
                start(
                    u,
                    sender,
                    Job::SetActions(true),
                    "Đang bật dịch vụ nhận phím…",
                )?;
            } else if terminal && !u.get_status_error() {
                // Replace the "turning on" note once the agent answers; its state may still lag.
                status(
                    u,
                    if agent.enabled {
                        "Dịch vụ nhận phím đã bật"
                    } else {
                        label
                    },
                    false,
                );
            }
        }
        Event::Profiles { config, revision } => {
            let keep_form = host_form_dirty(u, s);
            let mut st = s.borrow_mut();
            if st.pending_host.is_some() {
                return Ok(());
            }
            let initial = !st.profiles_loaded;
            st.host = config;
            st.host_revision = revision;
            st.profiles_loaded = true;
            u.set_host_profiles(strings(
                st.host.profiles.iter().map(|p| p.name.clone()).collect(),
            ));
            let index = if st.host.profiles.is_empty() {
                -1
            } else if initial {
                0
            } else {
                u.get_host_profile_index()
                    .clamp(0, st.host.profiles.len() as i32 - 1)
            };
            drop(st);
            if !keep_form {
                show_host(u, s, index);
            } else {
                status(
                    u,
                    "Bộ hành động đã được cập nhật. Nội dung đang nhập vẫn được giữ và chưa lưu",
                    true,
                );
            }
            refresh(u, s);
            refresh_catalog(u, s);
        }
        Event::ProfilesSaved { config, revision } => {
            let mut st = s.borrow_mut();
            let (previous, description, undo) = st
                .pending_host
                .take()
                .context("Không có thao tác profile đang lưu")?;
            st.host = config;
            st.host_revision = revision;
            if undo {
                st.undo.pop();
            } else {
                push_undo(&mut st, Undo::Host(previous));
            }
            record(&mut st, &description);
            let next = st.after_host_save.take();
            let quitting = st.quit_after_save;
            drop(st);
            refresh(u, s);
            refresh_catalog(u, s);
            status(u, format!("{description} · Agent đã xác nhận"), false);
            if let Some(job) = next {
                start(u, sender, job, "Đang gửi yêu cầu tới Coban-mod Agent…")?;
            } else if quitting {
                slint::quit_event_loop()?;
            }
        }
        Event::TestResult(text) => {
            u.set_host_status(text.clone().into());
            status(u, text, false);
        }

        Event::Error(text, fatal) => {
            // Closing the app must not hang on an agent that is already gone.
            if s.borrow().stop_after_response {
                slint::quit_event_loop()?;
                return Ok(());
            }
            let quitting = s.borrow().quit_after_save || s.borrow().hide_after_save;
            {
                let mut st = s.borrow_mut();
                st.poll_pending = false;
                st.quit_after_save = false;
                st.hide_after_save = false;
                st.stop_after_response = false;
                st.after_host_save = None;
                st.after_device_write = None;
                st.apply_after_preview = false;
                u.set_layout_preview_pending(false);
                if st.screen_rendering
                    || st.screen_applying
                    || st.screen_loading
                    || st.screen_asset_target.is_some()
                {
                    u.set_layout_save_failed(st.screen_rendering || st.screen_applying);
                    st.screen_applying = false;
                    st.screen_rendering = false;
                    st.screen_loading = false;
                    st.screen_asset_target = None;
                    st.screen_preview_valid = false;
                    u.set_layout_status(text.clone().into());
                }
                if let Some((previous, _, _)) = st.pending_host.take() {
                    st.host = previous;
                }
            }
            u.set_host_status(text.clone().into());
            if fatal {
                s.borrow_mut().editor.fail();
                s.borrow_mut().pending_undo = false;
                u.set_stale(true);
                u.set_connected(false);
                u.set_can_undo(false);
            }
            status(u, text, true);
            refresh(u, s);
            refresh_catalog(u, s);
            if quitting {
                let _ = u.show();
            }
        }
    }
    if terminal && !u.get_busy() && !s.borrow().quit_after_save {
        let pending = s.borrow_mut().pending_screen_layer.take();
        if let Some(layer) = pending {
            request_layer(u, s, sender, layer)?;
        }
    }
    if terminal && !u.get_busy() && s.borrow().quit_after_save {
        slint::quit_event_loop()?;
    }
    #[cfg(target_os = "macos")]
    if terminal && !u.get_busy() && s.borrow().hide_after_save {
        s.borrow_mut().hide_after_save = false;
        macos::hide(u)?;
    }
    Ok(())
}
fn worker(rx: mpsc::Receiver<Job>, tx: mpsc::Sender<Event>, weak: slint::Weak<MainWindow>) {
    let mut client: Option<service::Client> = None;
    let deliver = |event| {
        let _ = tx.send(event);
        let w = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(u) = w.upgrade() {
                u.invoke_process_updates();
            }
        });
    };
    for job in rx {
        let login_job = matches!(job, Job::LoginStatus | Job::SetLogin(_));
        let result = (|| -> Result<Event> {
            match job {
                Job::LoginStatus => return Ok(Event::LoginStatus(service::login::status()?)),
                Job::SetLogin(enabled) => {
                    return Ok(Event::LoginChanged(service::login::set_enabled(enabled)?));
                }
                _ => {}
            }
            if client.is_none() {
                let connected = service::Client::ensure_running()?;
                deliver(connected.call(Job::Status)?);
                deliver(connected.call(Job::Profiles)?);
                if let Ok(status) = service::login::status() {
                    deliver(Event::LoginStatus(status));
                }
                client = Some(connected);
            }
            client.as_ref().unwrap().call(job.clone())
        })();
        match result {
            Ok(event) => {
                let failed = matches!(event, Event::Error(..));
                deliver(event);
                if failed
                    && let Some(client) = &client
                    && let Ok(event) = client.call(Job::Profiles)
                {
                    deliver(event);
                }
            }
            Err(error) => {
                if !login_job {
                    client = None;
                }
                deliver(Event::Error(format!("{error:#}"), !login_job));
                if login_job && let Ok(status) = service::login::status() {
                    deliver(Event::LoginStatus(status));
                }
            }
        }
    }
}
fn run_callback(f: impl FnOnce() -> Result<()>) -> Result<()> {
    f()
}
fn wire(ui: &MainWindow, state: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) {
    state.borrow_mut().sender = Some(tx.clone());
    macro_rules! on {($name:ident,|$u:ident,$st:ident,$send:ident $(,$a:ident)*| $body:block)=>{{let weak=ui.as_weak();let st=state.clone();let sender=tx.clone();ui.$name(move|$($a),*|{let Some($u)=weak.upgrade()else{return;};let $st=&st;let $send=&sender;let result=run_callback(||->Result<()>{$body;Ok(())});if let Err(e)=result{status(&$u,format!("{e:#}"),true);}});}}}
    on!(on_discover, |u, st, send| {
        let _ = st;
        start(&u, send, Job::Discover(true), "Đang tìm Bao-keyboard…")?;
    });
    on!(on_connect_device, |u, st, send| {
        let d = st
            .borrow()
            .devices
            .get(u.get_device_index().max(0) as usize)
            .cloned()
            .context("Chọn bàn phím để kết nối")?;
        u.set_connected(false);
        st.borrow_mut().active_path = Some(d.path.clone());
        start(&u, send, Job::Connect(d), "Đang đọc cấu hình bàn phím…")?;
    });
    on!(on_read_device, |u, st, send| {
        let _ = st;
        start(&u, send, Job::Read, "Đang đọc lại bàn phím…")?;
    });
    on!(on_change_page, |u, st, send, page| {
        finish_edits(&u, st, send)?;
        u.set_page(page);
        show_settings(&u, st, 0);
        if page == 7 {
            send.send(Job::LoginStatus)?;
        }
    });
    on!(on_select_layer, |u, st, send, index| {
        finish_edits(&u, st, send)?;
        let max = st.borrow().editor.current()?.layers as i32;
        ensure!((0..max).contains(&index), "Lớp phím không hợp lệ");
        st.borrow_mut().inspection_origin = u
            .get_follow_layer()
            .then_some(u.get_active_layer())
            .filter(|n| *n >= 0)
            .map(|n| n as u8);
        request_layer(&u, st, send, index as u8)?;
    });
    on!(on_follow_layer_change, |u, st, send, enabled| {
        u.set_follow_layer(enabled);
        st.borrow_mut().inspection_origin = None;
        send.send(Job::Status)?;
    });
    on!(on_confirm_layer, |u, st, send| {
        finish_edits(&u, st, send)?;
        ready(&u)?;
        let before = st.borrow().editor.current()?.clone();
        start(
            &u,
            send,
            Job::ConfirmLayer {
                layer: u.get_layer() as u8,
                before: Box::new(before),
            },
            "Đang cập nhật tự đổi ảnh theo lớp…",
        )?;
    });
    on!(on_select_key, |u, st, send, index| {
        finish_edits(&u, st, send)?;
        ensure!((0..14).contains(&index), "Vị trí không hợp lệ");
        u.set_key_panel(0);
        u.set_selected_key(index);
        refresh(&u, st);
    });
    on!(on_select_position, |u, st, send, index| {
        // One click on the Coban-mod page selects the key function and its screen tile.
        finish_edits(&u, st, send)?;
        ensure!((0..14).contains(&index), "Vị trí không hợp lệ");
        u.set_selected_key(index);
        let tile = match index {
            0 => Some(8),
            1 => Some(9),
            2..=9 => Some(index as usize - 2),
            _ => None,
        };
        match tile {
            Some(tile) => {
                st.borrow_mut().screen_tile = tile;
                show_screen(&u, st, None)?;
                if u.get_key_panel() >= 2 {
                    u.set_key_panel(1);
                }
            }
            None => u.set_key_panel(0),
        }
        refresh(&u, st);
    });
    on!(on_search_function, |u, st, send, text| {
        let _ = send;
        u.set_search(text);
        refresh_catalog(&u, st);
    });
    on!(on_select_category, |u, st, send, index| {
        let _ = send;
        u.set_category(index);
        u.set_search("".into());
        refresh_catalog(&u, st);
    });
    on!(on_select_key_group, |u, st, send, index| {
        let _ = send;
        u.set_key_group(index);
        refresh_catalog(&u, st);
    });
    on!(on_assign, |u, st, send, code| {
        ensure!((0..=65535).contains(&code), "Chức năng không hợp lệ");
        assign(&u, st, send, code as u16)?;
    });
    on!(on_key_name_commit, |u, st, send| {
        let _ = send;
        if u.get_page() == 0 {
            commit_display_names(&u, st)?;
        }
    });
    on!(on_macro_display_name_commit, |u, st, send| {
        let _ = send;
        if u.get_page() == 1 {
            commit_display_names(&u, st)?;
        }
    });
    on!(on_host_trigger_name_commit, |u, st, send| {
        let _ = send;
        if u.get_page() == 5 {
            // Save a completed action and its name in the same profile transaction.
            commit_host(&u, st)?;
            if !u.get_busy() {
                commit_display_names(&u, st)?;
            }
        }
    });
    on!(on_assign_text, |u, st, send, text| {
        assign(&u, st, send, macro_steps::parse_shortcut(text.as_str())?)?;
        u.set_chord_input("".into());
    });
    on!(on_undo, |u, st, send| {
        ready(&u)?;
        let last = st
            .borrow()
            .undo
            .last()
            .cloned()
            .context("Chưa có thay đổi để hoàn tác")?;
        match last {
            Undo::Device => {
                let req = st.borrow_mut().editor.begin_undo()?;
                st.borrow_mut().pending_undo = true;
                write_request(&u, st, send, req)?;
            }
            Undo::Host(cfg) => {
                persist_host(&u, st, cfg, "Đã hoàn tác hành động trên máy tính", true)?;
                let index = u
                    .get_host_profile_index()
                    .min(st.borrow().host.profiles.len() as i32 - 1);
                show_host(&u, st, index);
            }
        }
    });
    on!(on_select_macro, |u, st, send, index| {
        finish_edits(&u, st, send)?;
        u.set_step_index(-1);
        show_macro(&u, st, index.max(0) as usize);
    });
    on!(on_macro_name_commit, |u, st, send, name| {
        if st.borrow().rendering || u.get_busy() || !u.get_connected() || u.get_page() != 1 {
            return Ok(());
        }
        ensure!(
            name.len() <= 6 && name.bytes().all(|b| b.is_ascii_graphic() || b == b' '),
            "Tên trên bàn phím tối đa 6 ký tự không dấu"
        );
        let index = st.borrow().macro_view;
        change(
            &u,
            st,
            send,
            format!("Đã đổi tên macro {}", index + 1),
            |d| {
                ensure!(index < d.macro_names.len(), "Chọn macro");
                d.macro_names[index] = name.into();
                Ok(())
            },
        )?;
    });
    on!(on_select_step, |u, st, send, index| {
        finish_edits(&u, st, send)?;
        let step = st
            .borrow()
            .steps
            .get(index.max(0) as usize)
            .cloned()
            .context("Chọn bước macro")?;
        st.borrow_mut().rendering = true;
        u.set_step_index(index);
        u.set_step_kind(step.kind);
        u.set_step_text(step.text.into());
        st.borrow_mut().rendering = false;
    });
    on!(on_step_commit, |u, st, send, text| {
        if st.borrow().rendering
            || u.get_busy()
            || u.get_step_index() < 0
            || !u.get_connected()
            || u.get_page() != 1
        {
            return Ok(());
        }
        let mut steps = st.borrow().steps.clone();
        let i = u.get_step_index() as usize;
        ensure!(i < steps.len(), "Chọn bước macro");
        macro_steps::set_text(&mut steps, i, text.into())?;
        commit_steps(&u, st, send, steps)?;
    });
    on!(on_change_step_kind, |u, st, send, kind| {
        ensure!(
            (0..=2).contains(&kind),
            "Chọn văn bản, tổ hợp hoặc khoảng chờ"
        );
        let mut steps = edited_steps(&u, st)?;
        let step = steps
            .get_mut(u.get_step_index().max(0) as usize)
            .context("Chọn bước macro")?;
        step.kind = kind;
        step.text = if kind == 2 {
            "200"
        } else if kind == 0 {
            "Xin chao"
        } else {
            "Enter"
        }
        .into();
        commit_steps(&u, st, send, steps)?;
    });
    on!(on_add_step, |u, st, send, kind| {
        let mut steps = edited_steps(&u, st)?;
        steps.push(Step {
            kind,
            text: if kind == 0 {
                "Text"
            } else if kind == 2 {
                "200"
            } else {
                "Enter"
            }
            .into(),
        });
        let selected = steps.len() as i32 - 1;
        commit_steps(&u, st, send, steps)?;
        u.set_step_index(selected);
    });
    on!(on_remove_step, |u, st, send| {
        let mut steps = st.borrow().steps.clone();
        let i = u.get_step_index();
        ensure!(i >= 0 && (i as usize) < steps.len(), "Chọn bước macro");
        macro_steps::remove(&mut steps, i as usize)?;
        commit_steps(&u, st, send, steps)?;
    });
    on!(on_bind_macro, |u, st, send, index| {
        let macro_index = st.borrow().macro_view;
        let encoded = macro_steps::encode(&edited_steps(&u, st)?)?;
        let name = u.get_macro_name().to_string();
        u.set_selected_key(index);
        change(
            &u,
            st,
            send,
            format!(
                "Đã gán macro {} cho {}",
                macro_index + 1,
                position(index).to_lowercase()
            ),
            |d| {
                d.macro_buffer = macros::replace(
                    &d.macro_buffer,
                    d.macro_count as usize,
                    macro_index,
                    &encoded,
                )?;
                d.macro_names[macro_index] = name;
                d.set_binding(
                    u.get_layer() as usize,
                    index as usize,
                    0x7700 + macro_index as u16,
                )
            },
        )?;
    });
    on!(on_select_setting, |u, st, send, index| {
        let _ = send;
        show_settings(&u, st, index);
    });
    on!(on_set_field, |u, st, send, index, value| {
        let key = st
            .borrow()
            .setting_keys
            .get(u.get_setting_index().max(0) as usize)
            .cloned()
            .context("Chọn nhóm thiết lập")?;
        change(
            &u,
            st,
            send,
            "Đã lưu thiết lập vào bàn phím".into(),
            |d| {
                let specs = setting_specs();
                let spec = specs
                    .iter()
                    .find(|s| s.key == key)
                    .context("Chưa hỗ trợ thiết lập")?;
                ensure!(
                    index >= 0
                        && (index as usize) < spec.len
                        && (0..=spec.fields[index as usize].1 as i32).contains(&value),
                    "Giá trị vượt giới hạn"
                );
                d.settings
                    .iter_mut()
                    .find(|s| s.key == key)
                    .context("Thiết bị không có thiết lập này")?
                    .value[index as usize] = value as u8;
                Ok(())
            },
        )?;
    });
    on!(on_backup, |u, st, send| {
        let _ = st;
        ready(&u)?;
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Xuất bản sao Bao-keyboard")
            .set_file_name("Bao-keyboard.mk.json")
            .add_filter("Coban-mod backup", &["json"])
            .save_file()
        {
            start(&u, send, Job::Backup(path), "Đang đọc và xuất bản sao…")?;
        }
    });
    on!(on_restore, |u, st, send| {
        ready(&u)?;
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Khôi phục cấu hình bàn phím")
            .add_filter("Coban-mod backup", &["json"])
            .pick_file()
        {
            let mut target = storage::load(&path)?;
            let before = st.borrow().editor.current()?.clone();
            before.compatible(&target)?;
            target.device = before.device;
            let request = st
                .borrow_mut()
                .editor
                .begin(target, "Đã khôi phục cấu hình từ bản sao".into())?;
            write_request(&u, st, send, request)?;
        }
    });
    on!(on_sync_clock, |u, st, send| {
        let _ = st;
        ready(&u)?;
        start(&u, send, Job::Clock, "Đang đồng bộ giờ…")?;
    });
    on!(on_open_backups, |u, st, send| {
        let _ = (&u, st, send);
        let dir = storage::data_dir().join("backups");
        std::fs::create_dir_all(&dir)?;
        host::open_target(&dir.to_string_lossy())?;
    });
    on!(on_upload_image, |u, st, send| {
        ready(&u)?;
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Chọn ảnh cho bàn phím")
            .add_filter("Images", &["png", "jpg", "jpeg", "gif"])
            .pick_file()
        {
            let before = st.borrow().editor.current()?.clone();
            start(
                &u,
                send,
                Job::Image {
                    path,
                    before: Box::new(before),
                },
                "Đang xử lý và gửi ảnh/GIF…",
            )?;
        }
    });
    on!(on_screen_mode_change, |u, st, send| {
        let index = u.get_screen_mode_index();
        ensure!((0..5).contains(&index), "Chế độ màn hình không hợp lệ");
        change(
            &u,
            st,
            send,
            "Đã đổi chế độ màn hình".into(),
            |d| {
                let screen = d
                    .settings
                    .iter_mut()
                    .find(|v| v.key == "screen")
                    .context("Không có màn hình")?;
                screen.value[0] = [0, 1, 2, 3, 5][index as usize];
                Ok(())
            },
        )?;
    });
    on!(on_screen_source_change, |u, st, send| {
        if st.borrow().screen_loaded {
            commit_screen(&u, st, send)?;
        } else if !u.get_busy() {
            start(
                &u,
                send,
                Job::LoadScreen {
                    layer: st.borrow().screen_layer,
                },
                "Đang tải bố cục…",
            )?;
        }
    });
    on!(on_layout_commit, |u, st, send| {
        commit_screen(&u, st, send)?;
    });
    on!(on_layout_kind_change, |u, st, send| {
        ready(&u)?;
        let tile = st
            .borrow()
            .screen_layout
            .tile(st.borrow().screen_tile)?
            .clone();
        if u.get_layout_kind() == 1 && tile.image.is_none() {
            u.invoke_layout_pick_image(false);
            if !u.get_busy() {
                show_screen(&u, st, None)?;
            }
        } else {
            commit_screen(&u, st, send)?;
        }
    });
    on!(on_layout_select_tile, |u, st, send, index| {
        ensure!((0..=10).contains(&index), "Chọn phím, núm xoay hoặc ô giữa");
        commit_screen(&u, st, send)?;
        st.borrow_mut().screen_tile = index as usize;
        if u.get_page() == 0 {
            // The centre strip has no key function, only its own artwork panel.
            u.set_key_panel(if index == 10 { 3 } else { 1 });
        }
        show_screen(&u, st, None)?;
    });
    on!(on_layout_select_background, |u, st, send| {
        commit_screen(&u, st, send)?;
        if u.get_page() == 0 {
            u.set_key_panel(2);
        }
    });
    on!(on_layout_bg_change, |u, st, send| {
        if u.get_layout_bg_kind() == 1 && st.borrow().screen_layout.background_image.is_none() {
            u.invoke_layout_pick_image(true);
            if !u.get_busy() {
                show_screen(&u, st, None)?;
            }
        } else {
            commit_screen(&u, st, send)?;
        }
    });
    on!(on_layout_preset_color, |u, st, send, color| {
        u.set_layout_bg_color(color);
        commit_screen(&u, st, send)?;
    });
    on!(on_layout_pick_image, |u, st, send, background| {
        ready(&u)?;
        if let Some(path) = rfd::FileDialog::new()
            .set_title(if background {
                "Chọn ảnh nền"
            } else {
                "Chọn ảnh cho phím / núm xoay"
            })
            .add_filter("Ảnh", &["png", "jpg", "jpeg", "gif"])
            .pick_file()
        {
            start(
                &u,
                send,
                Job::ImportScreenAsset { path, background },
                "Đang xử lý ảnh trên máy…",
            )?;
            st.borrow_mut().screen_asset_target = Some(background);
        }
    });
    on!(on_layout_pick_symbol, |u, st, send, name| {
        u.set_layout_symbol_name(name);
        commit_screen(&u, st, send)?;
    });
    on!(on_layout_symbol_search, |u, st, send, text| {
        let _ = (st, send);
        u.set_layout_symbol_query(text);
        show_symbol_choices(&u)?;
    });
    on!(on_layout_symbol_weight_change, |u, st, send| {
        show_symbol_choices(&u)?;
        commit_screen(&u, st, send)?;
    });
    on!(on_layout_apply, |u, st, send| {
        if st.borrow().screen_rendering {
            st.borrow_mut().apply_after_preview = true;
            return Ok(());
        }
        ready(&u)?;
        let cfg = screen_form(&u, st)?;
        let force = !st.borrow().screen_preview_valid;
        if preview_screen(&u, st, send, cfg, force)? {
            st.borrow_mut().apply_after_preview = true;
        } else {
            apply_screen(&u, st, send)?;
        }
    });
    on!(on_layout_reload, |u, st, send| {
        st.borrow_mut().apply_after_preview = false;
        start(
            &u,
            send,
            Job::LoadScreen {
                layer: st.borrow().screen_layer,
            },
            "Đang tải lại bố cục đã lưu…",
        )?;
    });
    on!(on_host_select, |u, st, send, index| {
        let _ = send;
        commit_host(&u, st)?;
        if !u.get_busy() {
            commit_display_names(&u, st)?;
        }
        show_host(&u, st, index);
    });
    on!(on_host_new, |u, st, send| {
        let _ = send;
        let mut cfg = st.borrow().host.clone();
        cfg.profiles.push(HostProfile {
            name: format!("Bộ hành động {}", cfg.profiles.len() + 1),
            application: String::new(),
            bindings: Vec::new(),
        });
        let idx = cfg.profiles.len() as i32 - 1;
        persist_host(&u, st, cfg, "Đã tạo bộ hành động trên máy tính", false)?;
        show_host(&u, st, idx);
    });
    on!(on_host_commit, |u, st, send| {
        let _ = send;
        commit_host(&u, st)?;
    });
    on!(on_host_change_kind, |u, st, send| {
        let _ = send;
        let text = u.get_host_text();
        let incompatible = match u.get_host_kind() {
            1 => host::normalize_web_target(&text).is_err(),
            2 => host::validate_application_target(&text).is_err(),
            3 => text.contains("://") || !std::path::Path::new(text.as_str()).exists(),
            _ => false,
        };
        if incompatible {
            u.set_host_text("".into());
            u.set_host_status(
                match u.get_host_kind() {
                    1 => "Nhập địa chỉ website.",
                    2 => "Chọn ứng dụng cần mở.",
                    _ => "Chọn tệp cần mở.",
                }
                .into(),
            );
        } else {
            commit_host(&u, st)?;
        }
    });
    on!(on_host_open_permissions, |u, st, send| {
        let _ = st;
        start(
            &u,
            send,
            Job::SetupInput,
            "Đang thiết lập nhận phím cho Coban-mod Agent…",
        )?;
    });
    on!(on_host_remove, |u, st, send| {
        let _ = send;
        let mut cfg = st.borrow().host.clone();
        let p = cfg
            .profiles
            .get_mut(u.get_host_profile_index().max(0) as usize)
            .context("Chọn bộ hành động")?;
        p.bindings
            .retain(|b| b.trigger != 104 + u.get_host_trigger() as u16);
        persist_host(&u, st, cfg, "Đã xóa hành động trên máy tính", false)?;
        show_host(&u, st, u.get_host_profile_index());
    });
    on!(on_host_pick_app, |u, st, send| {
        let _ = send;
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Chọn ứng dụng áp dụng bộ hành động")
            .pick_file()
        {
            u.set_host_app(host::application_identity(&path)?.into());
            commit_host(&u, st)?;
        }
    });
    on!(on_host_pick_target, |u, st, send| {
        let _ = send;
        let dialog = rfd::FileDialog::new().set_title(if u.get_host_kind() == 2 {
            "Chọn ứng dụng cần mở"
        } else {
            "Chọn tệp cần mở"
        });
        #[cfg(target_os = "macos")]
        let dialog = if u.get_host_kind() == 2 {
            dialog
                .set_directory("/Applications")
                .add_filter("Ứng dụng", &["app"])
        } else {
            dialog
        };
        #[cfg(target_os = "windows")]
        let dialog = if u.get_host_kind() == 2 {
            dialog.add_filter("Ứng dụng", &["exe", "lnk"])
        } else {
            dialog
        };
        if let Some(path) = dialog.pick_file() {
            u.set_host_text(path.to_string_lossy().to_string().into());
            commit_host(&u, st)?;
        }
    });
    on!(on_host_test, |u, st, send| {
        ensure!(
            !u.get_host_text().trim().is_empty(),
            "Nhập nội dung hành động trước khi thử"
        );
        commit_host(&u, st)?;
        let job = Job::TestAction {
            profile: u.get_host_profile_index().max(0) as usize,
            trigger: 104 + u.get_host_trigger() as u16,
        };
        if st.borrow().pending_host.is_some() {
            st.borrow_mut().after_host_save = Some(job);
        } else {
            start(&u, send, job, "Đang thử hành động qua Coban-mod Agent…")?;
        }
    });
    on!(on_host_toggle, |u, st, send| {
        commit_host(&u, st)?;
        let job = Job::SetActions(!u.get_host_enabled());
        if st.borrow().pending_host.is_some() {
            st.borrow_mut().after_host_save = Some(job);
        } else {
            start(&u, send, job, "Đang cập nhật dịch vụ nhận phím…")?;
        }
    });
    on!(on_host_login_toggle, |u, st, send, enabled| {
        let _ = st;
        start(
            &u,
            send,
            Job::SetLogin(enabled),
            "Đang cập nhật tùy chọn đăng nhập…",
        )?;
    });
    on!(on_stop_agent, |u, st, send| {
        finish_edits(&u, st, send)?;
        st.borrow_mut().stop_after_response = true;
        if st.borrow().pending_host.is_some() {
            st.borrow_mut().after_host_save = Some(Job::StopAgent);
        } else if u.get_busy() {
            st.borrow_mut().after_device_write = Some(Job::StopAgent);
        } else {
            start(&u, send, Job::StopAgent, "Đang dừng dịch vụ…")?;
        }
    });
}
fn show_window(ui: &MainWindow) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::show(ui);
    #[cfg(not(target_os = "macos"))]
    Ok(ui.show()?)
}
pub fn run(background: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    let instance = {
        let Some(instance) = macos::Instance::acquire(background)? else {
            return Ok(());
        };
        macos::select_backend(background)?;
        Rc::new(instance)
    };
    let ui = MainWindow::new()?;
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_is_macos(cfg!(target_os = "macos"));
    let tray = MKTray::new()?;
    #[cfg(target_os = "macos")]
    let _reopen_handler = macos::install_reopen_handler(&ui);
    let state = Rc::new(RefCell::new(State::default()));
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    let weak = ui.as_weak();
    std::thread::spawn(move || worker(rx, etx, weak));
    wire(&ui, &state, &tx);
    {
        let w = ui.as_weak();
        let s = state.clone();
        let sender = tx.clone();
        ui.on_process_updates(move || {
            if let Some(u) = w.upgrade() {
                for event in erx.try_iter() {
                    if let Err(e) = handle_event(&u, &s, &sender, event) {
                        s.borrow_mut().editor.fail();
                        u.set_stale(true);
                        u.set_connected(false);
                        u.set_can_undo(false);
                        status(&u, format!("{e:#}"), true);
                    }
                }
            }
        });
    }
    ui.set_bindings(model(
        (0..14)
            .map(|_| {
                view(presentation::Function {
                    code: 0,
                    title: "Chưa kết nối".into(),
                    detail: String::new(),
                    icon: 0,
                })
            })
            .collect(),
    ));
    refresh_catalog(&ui, &state);
    ui.set_host_profiles(strings(
        state
            .borrow()
            .host
            .profiles
            .iter()
            .map(|p| p.name.clone())
            .collect(),
    ));
    let host_index = if state.borrow().host.profiles.is_empty() {
        -1
    } else {
        0
    };
    show_host(&ui, &state, host_index);
    {
        let weak = ui.as_weak();
        tray.on_open_editor(move || {
            if let Some(u) = weak.upgrade() {
                let _ = show_window(&u);
            }
        });
    }
    {
        let weak = ui.as_weak();
        tray.on_open_settings(move || {
            if let Some(u) = weak.upgrade() {
                u.invoke_change_page(7);
                let _ = show_window(&u);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        let quit_sender = tx.clone();
        tray.on_quit_app(move || {
            if let Some(u) = weak.upgrade()
                && u.get_busy()
            {
                status(
                    &u,
                    "Đang lưu lên bàn phím; chờ hoàn tất trước khi thoát.",
                    true,
                );
                let _ = u.show();
                return;
            }
            if let Some(u) = weak.upgrade() {
                if let Err(e) = finish_edits(&u, &s, &quit_sender) {
                    status(&u, format!("{e:#}"), true);
                    let _ = u.show();
                    return;
                }
                if u.get_busy() {
                    s.borrow_mut().quit_after_save = true;
                    return;
                }
                // Quitting the app also ends Coban-mod Agent and its key service.
                u.invoke_stop_agent();
                return;
            }
            let _ = slint::quit_event_loop();
        });
    }
    {
        let weak = ui.as_weak();
        let close_state = state.clone();
        let close_sender = tx.clone();
        ui.window().on_close_requested(move || {
            if let Some(u) = weak.upgrade() {
                if let Err(e) = finish_edits(&u, &close_state, &close_sender) {
                    status(&u, format!("{e:#}"), true);
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
                if u.get_busy() {
                    #[cfg(target_os = "macos")]
                    {
                        close_state.borrow_mut().hide_after_save = true;
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        close_state.borrow_mut().quit_after_save = true;
                    }
                } else {
                    #[cfg(target_os = "macos")]
                    let _ = macos::hide(&u);
                    #[cfg(not(target_os = "macos"))]
                    u.invoke_stop_agent();
                }
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }
    let timer = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let sender = tx.clone();
        let state = state.clone();
        #[cfg(target_os = "macos")]
        let instance = instance.clone();
        let ticks = std::cell::Cell::new(0u32);
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(1),
            move || {
                if let Some(u) = weak.upgrade() {
                    #[cfg(target_os = "macos")]
                    if instance.take_open_request() {
                        let _ = show_window(&u);
                    }
                    u.set_clock_text(chrono::Local::now().format("%H:%M").to_string().into());
                    let tick = ticks.get().wrapping_add(1);
                    ticks.set(tick);
                    let mut st = state.borrow_mut();
                    if !u.get_busy() && !st.poll_pending && !st.stop_after_response {
                        st.poll_pending = true;
                        let _ = sender.send(Job::Status);
                        if tick.is_multiple_of(5) {
                            let _ = sender.send(Job::Discover(false));
                        }
                    }
                    let stopping = st.stop_after_response;
                    drop(st);
                    if !stopping {
                        clock_tick(&u, &state, &sender);
                    }
                }
            },
        );
    }
    if !background {
        show_window(&ui)?;
    }
    tray.show()?;
    start(&ui, &tx, Job::Discover(true), "Đang tìm Bao-keyboard…")?;
    slint::run_event_loop()?;
    Ok(())
}

#[cfg(feature = "internal-ui-qa")]
#[allow(dead_code)]
pub fn run_qa() -> Result<()> {
    use mk::protocol::{MockTransport, Protocol};
    ensure!(
        std::env::var_os("MK_DATA_DIR").is_some(),
        "UI QA cần thư mục dữ liệu kiểm thử riêng (MK_DATA_DIR)"
    );
    let ui = MainWindow::new()?;
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_is_macos(cfg!(target_os = "macos"));
    ui.set_window_title("Coban-mod · UI QA · Dữ liệu kiểm thử".into());
    let state = Rc::new(RefCell::new(State::default()));
    let mut protocol = Protocol::new(MockTransport::new());
    let snapshot = protocol.read_snapshot(protocol.io.snapshot.device.clone())?;
    state.borrow_mut().editor.replace(snapshot)?;
    state.borrow_mut().ever_connected = true;
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        for job in rx {
            let result = (|| -> Result<Event> {
                match job {
                    Job::Write(c) => Ok(Event::Written(protocol.apply(&c.before, &c.after)?)),
                    Job::Read => Ok(Event::Snapshot(
                        protocol.read_snapshot(protocol.io.snapshot.device.clone())?,
                    )),
                    Job::UpdateProfiles {
                        expected_revision,
                        config,
                    } => Ok(Event::ProfilesSaved {
                        config,
                        revision: expected_revision + 1,
                    }),
                    Job::Backup(path) => {
                        storage::save(&path, &protocol.io.snapshot)?;
                        Ok(Event::Backup(path))
                    }
                    Job::LoadScreen { layer } => {
                        let stored = screen_layout::Store::load_layer(layer)?;
                        let rendered = screen_layout::render(&stored.layout)?;
                        Ok(Event::ScreenDraft {
                            layer,
                            revision: stored.revision,
                            layout: stored.layout,
                            png: rendered.png,
                            preview_png: rendered.preview_png,
                        })
                    }
                    Job::RenderScreen {
                        layer,
                        expected_revision,
                        layout,
                    } => {
                        let rendered = screen_layout::render(&layout)?;
                        let revision = expected_revision + 1;
                        screen_layout::Store {
                            revision,
                            layout: layout.clone(),
                        }
                        .save_layer(layer)?;
                        Ok(Event::ScreenPreview {
                            layer,
                            revision,
                            layout,
                            png: rendered.png,
                            preview_png: rendered.preview_png,
                        })
                    }
                    Job::ImportScreenAsset { path, background } => Ok(Event::ScreenAsset(
                        screen_layout::import_asset(&path, background)?,
                    )),
                    Job::ApplyScreen {
                        layer,
                        revision: _,
                        before,
                    } => {
                        let stored = screen_layout::Store::load_layer(layer)?;
                        let rendered = screen_layout::render(&stored.layout)?;
                        let result = protocol.apply_screen_gif(&before, &rendered.gif)?;
                        let output = storage::data_dir().join("qa-screen.gif");
                        storage::write_private(&output, &rendered.gif)?;
                        Ok(Event::ScreenApplied { result, output })
                    }
                    _ => bail!("QA không mở kết nối phần cứng"),
                }
            })();
            let _ = etx.send(result.unwrap_or_else(|e| Event::Error(format!("{e:#}"), true)));
            let w = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    u.invoke_process_updates();
                }
            });
        }
    });
    wire(&ui, &state, &tx);
    {
        let w = ui.as_weak();
        let s = state.clone();
        let sender = tx.clone();
        ui.on_process_updates(move || {
            if let Some(u) = w.upgrade() {
                for event in erx.try_iter() {
                    if let Err(e) = handle_event(&u, &s, &sender, event) {
                        status(&u, format!("{e:#}"), true);
                    }
                }
            }
        });
    }
    ui.set_connected(true);
    refresh(&ui, &state);
    refresh_catalog(&ui, &state);
    show_macro(&ui, &state, 0);
    show_settings(&ui, &state, 0);
    tx.send(Job::LoadScreen { layer: 0 })?;
    status(&ui, "UI QA · dữ liệu kiểm thử, không kết nối HID", false);
    // Screenshot helpers for the QA build only.
    if let Some(theme) = std::env::var("MK_QA_THEME")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        ui.set_theme_mode(theme);
    }
    if let Some(page) = std::env::var("MK_QA_PAGE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        ui.invoke_change_page(page);
    }
    if let Some(tile) = std::env::var("MK_QA_TILE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        ui.invoke_layout_select_tile(tile);
    }
    if let Some(panel) = std::env::var("MK_QA_PANEL")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        ui.set_key_panel(panel);
    }
    ui.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mk::protocol::{MockTransport, Protocol};
    use slint::Model;
    type Fixture = (
        MainWindow,
        Rc<RefCell<State>>,
        mpsc::Sender<Job>,
        mpsc::Receiver<Job>,
        Protocol<MockTransport>,
    );
    fn fixture() -> Fixture {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let state = Rc::new(RefCell::new(State::default()));
        let mut p = Protocol::new(MockTransport::new());
        let baseline = p.read_snapshot(p.io.snapshot.device.clone()).unwrap();
        state.borrow_mut().editor.replace(baseline).unwrap();
        ui.set_connected(true);
        let (tx, rx) = mpsc::channel();
        wire(&ui, &state, &tx);
        refresh(&ui, &state);
        refresh_catalog(&ui, &state);
        show_macro(&ui, &state, 0);
        (ui, state, tx, rx, p)
    }
    fn complete(
        ui: &MainWindow,
        s: &Rc<RefCell<State>>,
        tx: &mpsc::Sender<Job>,
        rx: &mpsc::Receiver<Job>,
        p: &mut Protocol<MockTransport>,
    ) {
        let Job::Write(c) = rx.try_recv().unwrap() else {
            panic!("Expected immediate write")
        };
        let result = p.apply(&c.before, &c.after).unwrap();
        handle_event(ui, s, tx, Event::Written(result)).unwrap();
    }
    #[test]
    fn selecting_function_writes_immediately_then_undo_writes_previous_value() {
        let (ui, s, tx, rx, mut p) = fixture();
        let baseline = s.borrow().editor.current().unwrap().clone();
        ui.invoke_select_layer(2);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::LoadScreen { layer: 2 }
        ));
        assert_eq!(p.io.writes, 0);
        let rendered = screen_layout::render(&Layout::default()).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenDraft {
                layer: 2,
                revision: 0,
                layout: Layout::default(),
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        ui.invoke_select_key(7);
        ui.invoke_assign(104);
        assert!(ui.get_busy());
        assert!(s.borrow().editor.current().unwrap().same_content(&baseline));
        complete(&ui, &s, &tx, &rx, &mut p);
        assert_eq!(s.borrow().editor.current().unwrap().binding(2, 7), 104);
        assert_eq!(ui.get_layer(), 2);
        assert_eq!(ui.get_selected_key(), 7);
        assert_eq!(ui.get_selected_function().title, "F13");
        assert!(ui.get_can_undo());
        ui.invoke_undo();
        complete(&ui, &s, &tx, &rx, &mut p);
        assert!(s.borrow().editor.current().unwrap().same_content(&baseline));
        assert!(!ui.get_can_undo());
        assert!(rx.try_recv().is_err());
    }
    fn agent(enabled: bool) -> Event {
        Event::HostStatus(service::AgentStatus {
            instance: "agent-1".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            enabled,
            state: if enabled {
                host::HostState::Ready
            } else {
                host::HostState::Stopped
            },
            message: String::new(),
            selected: None,
            profile_revision: 0,
            layer_sync: Default::default(),
        })
    }
    #[test]
    fn selecting_another_layer_loads_its_art_and_following_never_discards_an_edit() {
        let (ui, s, tx, rx, _) = fixture();
        let cfg = Layout::default();
        let rendered = screen_layout::render(&cfg).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenDraft {
                layer: 0,
                revision: 0,
                layout: cfg.clone(),
                png: rendered.png.clone(),
                preview_png: rendered.preview_png.clone(),
            },
        )
        .unwrap();
        ui.set_active_layer(0);
        ui.invoke_select_layer(2);
        assert!(ui.get_follow_layer());
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::LoadScreen { layer: 2 }
        ));
        let mut third = cfg.clone();
        third.tiles[0].text = "Lớp 3".into();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenDraft {
                layer: 2,
                revision: 3,
                layout: third.clone(),
                png: rendered.png.clone(),
                preview_png: rendered.preview_png.clone(),
            },
        )
        .unwrap();
        assert_eq!(ui.get_layer(), 2);
        assert_eq!(s.borrow().screen_layer, 2);
        assert_eq!(s.borrow().screen_layout, third);
        assert!(
            rx.try_recv().is_err(),
            "selection alone never sends artwork or keymap"
        );

        let Event::HostStatus(mut inspecting) = agent(true) else {
            unreachable!()
        };
        inspecting.layer_sync.active = Some(0);
        handle_event(&ui, &s, &tx, Event::HostStatus(inspecting)).unwrap();
        assert!(
            rx.try_recv().is_err(),
            "manual inspection stays until the device changes layer"
        );

        ui.set_follow_layer(true);
        ui.invoke_select_position(2);
        ui.set_key_panel(1);
        ui.set_layout_text("Chưa rời ô".into());
        let Event::HostStatus(mut status) = agent(true) else {
            unreachable!()
        };
        status.layer_sync.active = Some(1);
        handle_event(&ui, &s, &tx, Event::HostStatus(status)).unwrap();
        let Job::RenderScreen { layer, layout, .. } = rx.try_recv().unwrap() else {
            panic!("save the old layer first")
        };
        assert_eq!(layer, 2);
        assert_eq!(layout.tiles[0].text, "Chưa rời ô");
        assert_eq!(s.borrow().pending_screen_layer, Some(1));
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 2,
                revision: 4,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::ApplyScreen { layer: 2, .. }
        ));
        let baseline = s.borrow().editor.current().unwrap().clone();
        handle_event(&ui, &s, &tx, Event::ScreenStored { layer: 2 }).unwrap();
        assert!(s.borrow().editor.current().unwrap().same_content(&baseline));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::LoadScreen { layer: 1 }
        ));
    }
    #[test]
    fn key_service_starts_by_itself_once_after_the_keyboard_is_connected() {
        let (ui, s, tx, rx, _) = fixture();
        ui.set_connected(false);
        handle_event(&ui, &s, &tx, agent(false)).unwrap();
        assert!(rx.try_recv().is_err(), "no key service without a keyboard");
        ui.set_connected(true);
        handle_event(&ui, &s, &tx, agent(false)).unwrap();
        assert!(matches!(rx.try_recv().unwrap(), Job::SetActions(true)));
        handle_event(
            &ui,
            &s,
            &tx,
            Event::HostChanged(match agent(true) {
                Event::HostStatus(a) => a,
                _ => unreachable!(),
            }),
        )
        .unwrap();
        assert!(ui.get_host_running());
        // A later poll with the service stopped does not fight the user/agent in a loop.
        handle_event(&ui, &s, &tx, agent(false)).unwrap();
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn quitting_stops_the_agent_and_exits_even_when_the_agent_is_gone() {
        let (ui, s, tx, rx, _) = fixture();
        ui.invoke_stop_agent();
        assert!(matches!(rx.try_recv().unwrap(), Job::StopAgent));
        assert!(s.borrow().stop_after_response);
        // The test backend has no event loop, so the attempted quit is the error itself.
        let quit = handle_event(&ui, &s, &tx, Event::Error("agent gone".into(), true));
        assert!(format!("{:#}", quit.unwrap_err()).contains("event loop"));
        assert!(
            !ui.get_status_error(),
            "closing never surfaces the agent error"
        );
    }
    #[test]
    fn one_click_selects_the_key_function_and_its_screen_tile_without_writing() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_page(0);
        ui.invoke_select_position(7);
        assert_eq!(ui.get_selected_key(), 7);
        assert_eq!(ui.get_layout_tile(), 5);
        assert_eq!(ui.get_layout_cell_label(), "Phím 6");
        assert_eq!(ui.get_key_panel(), 0);
        ui.set_key_panel(1);
        ui.invoke_select_position(1);
        assert_eq!(ui.get_layout_cell_label(), "Núm phải");
        assert_eq!(
            ui.get_key_panel(),
            1,
            "the image tab stays open while moving between keys"
        );
        ui.invoke_select_position(12);
        assert_eq!(ui.get_key_panel(), 0, "knob turns have no screen tile");
        assert_eq!(p.io.writes, 0);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn startup_setting_works_without_keyboard_and_does_not_enable_input() {
        let (ui, s, tx, rx, _) = fixture();
        ui.set_connected(false);
        ui.invoke_change_page(7);
        assert_eq!(ui.get_page(), 7);
        assert!(matches!(rx.try_recv().unwrap(), Job::LoginStatus));
        ui.invoke_host_login_toggle(true);
        assert!(matches!(rx.try_recv().unwrap(), Job::SetLogin(true)));
        handle_event(
            &ui,
            &s,
            &tx,
            Event::LoginChanged(service::login::LoginStatus {
                available: true,
                enabled: true,
                message: "Đã bật khởi động cùng macOS".into(),
            }),
        )
        .unwrap();
        assert!(ui.get_host_login_enabled());
        assert!(!ui.get_busy());
        assert!(!ui.get_host_enabled());
        assert!(rx.try_recv().is_err());
        assert!(s.borrow().undo.is_empty());
    }
    #[test]
    fn macro_steps_save_without_apply_and_invalid_text_sends_no_write() {
        let (ui, s, tx, rx, mut p) = fixture();
        ui.invoke_change_page(1);
        ui.invoke_add_step(0);
        complete(&ui, &s, &tx, &rx, &mut p);
        assert_eq!(ui.get_step_text(), "Text");
        ui.invoke_step_commit("hello".into());
        complete(&ui, &s, &tx, &rx, &mut p);
        let d = s.borrow().editor.current().unwrap().clone();
        assert_eq!(
            macros::split(&d.macro_buffer, d.macro_count as usize).unwrap()[0],
            b"hello"
        );
        ui.invoke_step_commit("Tiếng Việt".into());
        assert!(ui.get_status_error());
        assert!(rx.try_recv().is_err());
        assert!(ui.get_connected());
        handle_event(&ui, &s, &tx, Event::Error("Disconnected".into(), true)).unwrap();
        ui.invoke_assign(115);
        assert!(rx.try_recv().is_err());
        assert!(!ui.get_connected());
        assert!(!ui.get_can_undo());
    }
    #[test]
    fn navigation_flushes_text_to_the_macro_being_edited() {
        let (ui, s, tx, rx, mut p) = fixture();
        ui.invoke_change_page(1);
        ui.invoke_add_step(0);
        complete(&ui, &s, &tx, &rx, &mut p);
        ui.set_step_text("Typed without leaving the field".into());
        // ComboBox updates its bound index before the selection callback.
        ui.set_macro_index(1);
        ui.invoke_select_macro(1);
        let Job::Write(c) = rx.try_recv().unwrap() else {
            panic!("Expected autosave")
        };
        let contents = macros::split(&c.after.macro_buffer, c.after.macro_count as usize).unwrap();
        assert_eq!(contents[0], b"Typed without leaving the field");
        assert!(contents[1].is_empty());
        let result = p.apply(&c.before, &c.after).unwrap();
        handle_event(&ui, &s, &tx, Event::Written(result)).unwrap();
        assert_eq!(ui.get_macro_index(), 1);
        assert_eq!(s.borrow().macro_view, 1);
    }
    #[test]
    fn selecting_a_saved_host_action_does_not_rewrite_or_reorder_bindings() {
        let (ui, s, _tx, rx, _p) = fixture();
        s.borrow_mut().host = HostConfig {
            display_names: Default::default(),
            profiles: vec![HostProfile {
                name: "Web".into(),
                application: String::new(),
                bindings: vec![
                    HostBinding {
                        trigger: 104,
                        action: HostAction::Web("facebook.com".into()),
                    },
                    HostBinding {
                        trigger: 105,
                        action: HostAction::Web("youtube.com".into()),
                    },
                ],
            }],
        };
        let before = serde_json::to_vec(&s.borrow().host).unwrap();
        ui.set_page(5);
        show_host(&ui, &s, 0);
        ui.set_host_trigger(1);
        ui.invoke_host_select(0);
        assert_eq!(ui.get_host_text(), "youtube.com");
        assert_eq!(serde_json::to_vec(&s.borrow().host).unwrap(), before);
        assert!(s.borrow().undo.is_empty());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn switching_from_website_to_application_saves_a_distinct_action_without_usb_writes() {
        let (ui, s, tx, rx, _p) = fixture();
        s.borrow_mut().host = HostConfig {
            display_names: Default::default(),
            profiles: vec![HostProfile {
                name: "Web and apps".into(),
                application: String::new(),
                bindings: vec![HostBinding {
                    trigger: 104,
                    action: HostAction::Web("youtube.com".into()),
                }],
            }],
        };
        ui.set_page(5);
        show_host(&ui, &s, 0);
        assert_eq!(ui.get_host_kind(), 1);
        ui.set_host_kind(2);
        ui.invoke_host_change_kind();
        assert!(ui.get_host_text().is_empty());
        ui.invoke_host_commit();
        assert!(matches!(
            s.borrow().host.action("", 104),
            Some(HostAction::Web(_))
        ));
        ui.set_host_text("Safari".into());
        ui.invoke_host_commit();
        assert!(
            matches!(s.borrow().host.action("",104),Some(HostAction::Application(t)) if t=="Safari")
        );
        show_host(&ui, &s, 0);
        assert_eq!(ui.get_host_kind(), 2);
        assert_eq!(ui.get_host_text(), "Safari");
        assert!(ui.get_busy());
        assert!(s.borrow().undo.is_empty());
        let Job::UpdateProfiles {
            expected_revision,
            config,
        } = rx.try_recv().unwrap()
        else {
            panic!("expected agent profile update");
        };
        assert_eq!(expected_revision, 0);
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ProfilesSaved {
                config,
                revision: 1,
            },
        )
        .unwrap();
        assert!(!ui.get_busy());
        assert_eq!(s.borrow().host_revision, 1);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn failed_profile_save_restores_committed_model_and_never_records_success() {
        let (ui, s, tx, rx, _) = fixture();
        ui.set_page(5);
        show_host(&ui, &s, -1);
        ui.set_host_text("example.com".into());
        ui.invoke_host_commit();
        assert!(matches!(rx.try_recv().unwrap(), Job::UpdateProfiles { .. }));
        assert!(ui.get_busy());
        assert!(s.borrow().undo.is_empty());
        handle_event(&ui, &s, &tx, Event::Error("Profile conflict".into(), false)).unwrap();
        assert!(s.borrow().host.profiles.is_empty());
        assert!(s.borrow().undo.is_empty());
        assert!(s.borrow().pending_host.is_none());
        assert!(ui.get_status_error());
        assert!(ui.get_connected());
    }
    #[test]
    fn action_test_waits_for_agent_profile_acknowledgement() {
        let (ui, s, tx, rx, _) = fixture();
        ui.set_page(5);
        show_host(&ui, &s, -1);
        ui.set_host_text("example.com".into());
        ui.invoke_host_test();
        let Job::UpdateProfiles { config, .. } = rx.try_recv().unwrap() else {
            panic!("expected profile update first");
        };
        assert!(rx.try_recv().is_err());
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ProfilesSaved {
                config,
                revision: 1,
            },
        )
        .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::TestAction {
                profile: 0,
                trigger: 104
            }
        ));
        assert!(ui.get_busy());
        handle_event(&ui, &s, &tx, Event::TestResult("done".into())).unwrap();
        assert!(!ui.get_busy());
    }
    #[test]
    fn remote_profile_refresh_keeps_uncommitted_form_text() {
        let (ui, s, tx, _, _) = fixture();
        ui.set_page(5);
        show_host(&ui, &s, -1);
        ui.set_host_text("unsaved.example.com".into());
        let config = HostConfig {
            display_names: Default::default(),
            profiles: vec![HostProfile {
                name: "remote".into(),
                application: String::new(),
                bindings: vec![],
            }],
        };
        handle_event(
            &ui,
            &s,
            &tx,
            Event::Profiles {
                config,
                revision: 2,
            },
        )
        .unwrap();
        assert_eq!(ui.get_host_text(), "unsaved.example.com");
        assert_eq!(s.borrow().host_revision, 2);
        assert!(s.borrow().pending_host.is_none());
    }
    #[test]
    fn empty_website_slot_allows_navigation_without_creating_a_binding() {
        let (ui, s, _tx, rx, _p) = fixture();
        s.borrow_mut().host = HostConfig {
            display_names: Default::default(),
            profiles: vec![HostProfile {
                name: "Empty".into(),
                application: String::new(),
                bindings: vec![],
            }],
        };
        ui.set_page(5);
        show_host(&ui, &s, 0);
        ui.invoke_change_page(0);
        assert_eq!(ui.get_page(), 0);
        assert!(s.borrow().host.profiles[0].bindings.is_empty());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn key_alias_updates_layout_search_and_trigger_picker_without_device_writes() {
        let (ui, s, tx, rx, mut p) = fixture();
        ui.invoke_assign(104);
        complete(&ui, &s, &tx, &rx, &mut p);
        let before = s.borrow().editor.current().unwrap().clone();
        ui.set_key_display_name("Mở bài giảng".into());
        ui.invoke_key_name_commit();
        assert!(ui.get_busy());
        let Job::UpdateProfiles { config, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(config.display_name(104), Some("Mở bài giảng"));
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ProfilesSaved {
                config,
                revision: 1,
            },
        )
        .unwrap();
        assert_eq!(ui.get_selected_function().title, "Mở bài giảng");
        assert!(ui.get_selected_function().detail.contains("F13"));
        assert_eq!(
            ui.get_host_triggers().row_data(0).unwrap(),
            "F13 · Mở bài giảng"
        );
        ui.invoke_search_function("bài giảng".into());
        assert_eq!(ui.get_functions().row_data(0).unwrap().code, 104);
        ui.invoke_search_function("F13".into());
        assert_eq!(
            ui.get_functions().row_data(0).unwrap().title,
            "Mở bài giảng"
        );
        assert!(before.same_content(s.borrow().editor.current().unwrap()));
        ui.invoke_undo();
        let Job::UpdateProfiles { config, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ProfilesSaved {
                config,
                revision: 2,
            },
        )
        .unwrap();
        assert_eq!(ui.get_selected_function().title, "F13");
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn macro_alias_navigation_keeps_firmware_name_and_edits_the_original_slot() {
        let (ui, s, tx, rx, _) = fixture();
        let before = s.borrow().editor.current().unwrap().clone();
        ui.invoke_change_page(1);
        ui.set_macro_display_name("Chèn lời giải".into());
        ui.set_macro_index(1);
        ui.invoke_select_macro(1);
        let Job::UpdateProfiles { config, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(config.display_name(0x7700), Some("Chèn lời giải"));
        assert!(config.display_name(0x7701).is_none());
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ProfilesSaved {
                config,
                revision: 1,
            },
        )
        .unwrap();
        assert!(ui.get_macro_display_name().is_empty());
        assert_eq!(
            ui.get_macro_list().row_data(0).unwrap(),
            "M0 · Chèn lời giải"
        );
        assert!(before.same_content(s.borrow().editor.current().unwrap()));
        ui.invoke_search_function("lời giải".into());
        assert_eq!(ui.get_functions().row_data(0).unwrap().code, 0x7700);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn trigger_alias_and_bare_website_save_together_and_failure_restores_names() {
        let (ui, s, tx, rx, _) = fixture();
        ui.set_page(5);
        show_host(&ui, &s, -1);
        ui.set_host_text("youtube.com/watch?v=lesson".into());
        ui.set_host_trigger_name("Xem bài giảng".into());
        ui.invoke_host_trigger_name_commit();
        let Job::UpdateProfiles { config, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(config.display_name(104), Some("Xem bài giảng"));
        let Some(HostAction::Web(target)) = config.action("", 104) else {
            panic!()
        };
        assert_eq!(
            host::normalize_web_target(target).unwrap(),
            "https://youtube.com/watch?v=lesson"
        );
        handle_event(&ui, &s, &tx, Event::Error("Save failed".into(), false)).unwrap();
        assert!(s.borrow().host.display_name(104).is_none());
        assert!(ui.get_host_trigger_name().is_empty());
        assert!(ui.get_status_error());
        assert!(rx.try_recv().is_err());
    }
    fn screen_fixture(ui: &MainWindow, s: &Rc<RefCell<State>>, tx: &mpsc::Sender<Job>) {
        ui.set_page(3);
        ui.set_screen_source(1);
        ui.set_screen_is_gif(true);
        let layout = Layout::default();
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            ui,
            s,
            tx,
            Event::ScreenDraft {
                layer: 0,
                revision: 0,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
    }
    #[test]
    fn screen_edits_auto_upload_and_tile_navigation_saves_the_original_tile() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_layout_text("Lời giải".into());
        ui.invoke_layout_select_tile(1);
        let Job::RenderScreen {
            layer: 0,
            layout,
            expected_revision: 0,
        } = rx.try_recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(layout.tiles[0].text, "Lời giải");
        assert_eq!(layout.tiles[1].text, "2");
        let png = screen_layout::render(&layout).unwrap().png;
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png,
                preview_png: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(ui.get_layout_text(), "2");
        assert_eq!(p.io.writes, 0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::ApplyScreen { revision: 1, .. }
        ));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn knob_selection_and_autosave_keep_the_other_knob_and_all_keys() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        let keys = s.borrow().screen_layout.tiles.clone();
        ui.invoke_layout_select_tile(8);
        assert_eq!(ui.get_layout_cell_label(), "Núm trái");
        ui.set_layout_text("VOL".into());
        ui.invoke_layout_select_tile(9);
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(layout.knobs[0].text, "VOL");
        assert_eq!(layout.knobs[1].text, "R");
        assert_eq!(layout.tiles, keys);
        let png = screen_layout::render(&layout).unwrap().png;
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png,
                preview_png: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(ui.get_layout_cell_label(), "Núm phải");
        assert_eq!(ui.get_layout_text(), "R");
        assert_eq!(p.io.writes, 0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::ApplyScreen { revision: 1, .. }
        ));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn cell_colour_and_icon_size_are_saved_with_the_selected_tile_and_preview_is_retina() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        let size = ui.get_layout_sharp_preview().size();
        assert_eq!((size.width, size.height), (640, 320));
        ui.set_layout_use_common_ink(false);
        ui.set_layout_cell_ink("#EF4444".into());
        ui.set_layout_icon_size(1);
        ui.invoke_layout_select_tile(8);
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(layout.tiles[0].ink, Some([239, 68, 68]));
        assert_eq!(layout.tiles[0].icon_size, screen_layout::IconSize::Medium);
        assert_eq!(layout.knobs[0].ink, None);
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        assert!(ui.get_layout_use_common_ink());
        assert_eq!(ui.get_layout_icon_size(), 0);
        assert_eq!(p.io.writes, 0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::ApplyScreen { revision: 1, .. }
        ));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn completed_edit_waits_for_render_then_auto_uploads_and_failure_never_replays() {
        let (ui, s, tx, rx, mut p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_layout_text("Apply me".into());
        ui.invoke_layout_commit();
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        // No Apply click: a finished edit must automatically continue to the hardware job.
        assert!(rx.try_recv().is_err());
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        let Job::ApplyScreen {
            layer: 0,
            revision: 1,
            before,
        } = rx.try_recv().unwrap()
        else {
            panic!()
        };
        let result = p.apply_screen_gif(&before, &rendered.gif).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenApplied {
                result,
                output: "test.gif".into(),
            },
        )
        .unwrap();
        assert!(!ui.get_busy());
        assert!(ui.get_screen_is_gif());
        ui.set_layout_text("New".into());
        ui.invoke_layout_commit();
        assert!(matches!(rx.try_recv().unwrap(), Job::RenderScreen { .. }));
        handle_event(&ui, &s, &tx, Event::Error("Render failed".into(), false)).unwrap();
        assert!(!s.borrow().apply_after_preview);
        assert!(rx.try_recv().is_err());
        assert!(ui.get_connected());
        assert!(ui.get_layout_save_failed());
        ui.invoke_layout_select_tile(2);
        ui.invoke_layout_select_background();
        ui.invoke_change_page(0);
        assert!(rx.try_recv().is_err());
        // An explicit retry is available after a failure; navigation never retries it.
        ui.invoke_layout_apply();
        assert!(matches!(rx.try_recv().unwrap(), Job::RenderScreen { .. }));
    }
    #[test]
    fn mini_screen_changes_the_sidebar_without_writing_and_physical_keys_restore_functions() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_page(0);
        ui.invoke_layout_select_tile(6);
        assert_eq!(ui.get_key_panel(), 1);
        assert_eq!(ui.get_layout_cell_label(), "Phím 7");
        ui.invoke_layout_select_background();
        assert_eq!(ui.get_key_panel(), 2);
        ui.invoke_layout_select_tile(9);
        assert_eq!(ui.get_key_panel(), 1);
        assert_eq!(ui.get_layout_cell_label(), "Núm phải");
        ui.invoke_select_key(11);
        assert_eq!(ui.get_key_panel(), 0);
        assert_eq!(ui.get_selected_key(), 11);
        assert_eq!(p.io.writes, 0);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn imported_artwork_auto_uploads_for_the_selected_key_and_keeps_other_tiles() {
        let (ui, s, tx, rx, _) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_page(0);
        ui.invoke_layout_select_tile(3);
        let original = s.borrow().screen_layout.clone();
        s.borrow_mut().screen_asset_target = Some(false);
        ui.set_busy(true);
        let png = screen_layout::png(&image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([255, 0, 0, 255]),
        ))
        .unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenAsset(screen_layout::Asset {
                name: "test.png".into(),
                png,
            }),
        )
        .unwrap();
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(layout.tiles[3].kind, Content::Image);
        assert_eq!(layout.tiles[3].image.as_ref().unwrap().name, "test.png");
        assert_eq!(&layout.tiles[..3], &original.tiles[..3]);
        assert_eq!(&layout.tiles[4..], &original.tiles[4..]);
        assert_eq!(layout.knobs, original.knobs);
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Job::ApplyScreen { revision: 1, .. }
        ));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn background_edits_auto_upload_but_loading_and_disconnected_edits_do_not() {
        let (ui, s, tx, rx, _) = fixture();
        screen_fixture(&ui, &s, &tx);
        assert!(rx.try_recv().is_err());
        ui.set_page(0);
        ui.invoke_layout_select_background();
        ui.invoke_layout_preset_color("#1E3A8A".into());
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(layout.background, [30, 58, 138]);
        let rendered = screen_layout::render(&layout).unwrap();
        // Losing the device during rendering must stop the upload chain.
        ui.set_connected(false);
        assert!(
            handle_event(
                &ui,
                &s,
                &tx,
                Event::ScreenPreview {
                    layer: 0,
                    revision: 1,
                    layout,
                    png: rendered.png,
                    preview_png: rendered.preview_png
                }
            )
            .is_err()
        );
        assert!(rx.try_recv().is_err());
        ui.set_layout_text("offline".into());
        ui.invoke_layout_commit();
        assert!(ui.get_status_error());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn failed_auto_upload_requires_a_read_and_never_replays_during_navigation() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_page(0);
        ui.invoke_layout_select_tile(0);
        ui.set_layout_text("NEW".into());
        ui.invoke_layout_commit();
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenPreview {
                layer: 0,
                revision: 1,
                layout,
                png: rendered.png,
                preview_png: rendered.preview_png,
            },
        )
        .unwrap();
        assert!(matches!(rx.try_recv().unwrap(), Job::ApplyScreen { .. }));
        handle_event(&ui, &s, &tx, Event::Error("USB disconnected".into(), true)).unwrap();
        assert!(!ui.get_connected());
        assert!(ui.get_stale());
        assert!(ui.get_layout_save_failed());
        assert_eq!(ui.get_layout_status(), "USB disconnected");
        ui.invoke_layout_apply();
        assert!(rx.try_recv().is_err());
        // A fresh read restores availability, without retrying the stored image.
        s.borrow_mut()
            .editor
            .replace(p.io.snapshot.clone())
            .unwrap();
        ui.set_connected(true);
        ui.set_stale(false);
        ui.invoke_layout_select_tile(1);
        ui.invoke_select_key(3);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn centre_tile_has_its_own_panel_and_phosphor_icons_upload() {
        let (ui, s, tx, rx, p) = fixture();
        screen_fixture(&ui, &s, &tx);
        ui.set_page(0);
        ui.invoke_layout_select_tile(10);
        assert_eq!(ui.get_key_panel(), 3);
        assert_eq!(ui.get_layout_cell_label(), "Ô giữa");
        assert_eq!(ui.get_layout_kind(), 4, "old layouts keep an empty centre");
        ui.invoke_layout_select_background();
        assert_eq!(ui.get_key_panel(), 2);
        ui.invoke_layout_select_tile(10);
        ui.set_layout_kind(2);
        ui.invoke_layout_symbol_search("youtube".into());
        assert!(
            ui.get_layout_symbol_choices()
                .iter()
                .any(|c| c.name == "youtube-logo")
        );
        assert!(rx.try_recv().is_err(), "searching never writes");
        ui.invoke_layout_pick_symbol("youtube-logo".into());
        let Job::RenderScreen { layout, .. } = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(layout.center.kind, Content::Symbol);
        assert_eq!(layout.center.symbol, "youtube-logo");
        assert_eq!(layout.center.symbol_weight, IconWeight::Fill);
        assert_eq!(layout.tiles, Layout::default().tiles);
        ui.invoke_select_position(3);
        assert_eq!(ui.get_key_panel(), 1);
        assert_eq!(p.io.writes, 0);
    }
    #[test]
    fn clock_ticks_once_a_minute_without_blocking_edits_and_pause_on_error() {
        let (ui, s, tx, rx, _) = fixture();
        let mut snapshot = s.borrow().editor.current().unwrap().clone();
        snapshot
            .settings
            .iter_mut()
            .find(|v| v.key == "screen")
            .unwrap()
            .value[0] = 2;
        s.borrow_mut().editor.replace(snapshot).unwrap();
        let mut layout = Layout::default();
        layout.center.kind = Content::Clock;
        let rendered = screen_layout::render(&layout).unwrap();
        handle_event(
            &ui,
            &s,
            &tx,
            Event::ScreenDraft {
                layer: 0,
                revision: 4,
                layout,
                png: rendered.png.clone(),
                preview_png: rendered.preview_png.clone(),
            },
        )
        .unwrap();
        clock_tick(&ui, &s, &tx);
        let Job::RefreshClock {
            layer: 0,
            revision: 4,
            at,
            screen,
        } = rx.try_recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(screen[0], 2);
        assert_eq!(chrono::Timelike::second(&at), 0);
        assert!(!ui.get_busy(), "a clock tick never locks the editor");
        clock_tick(&ui, &s, &tx);
        assert!(rx.try_recv().is_err(), "one tick in flight");
        // Another operation in flight keeps its busy state when the tick returns.
        ui.set_busy(true);
        let reply = |error: Option<&str>| Event::ClockRefreshed {
            layer: 0,
            revision: 4,
            png: rendered.png.clone(),
            preview_png: rendered.preview_png.clone(),
            millis: 900,
            error: error.map(Into::into),
        };
        handle_event(&ui, &s, &tx, reply(None)).unwrap();
        assert!(ui.get_busy());
        ui.set_busy(false);
        assert_eq!(s.borrow().clock_shown, Some(at));
        assert_eq!(s.borrow().clock_lead_ms, 1200);
        // Wall-clock independent: the minute due now (with the 1.2 s lead) is already shown.
        s.borrow_mut().clock_shown = Some(minute(
            chrono::Local::now().naive_local() + chrono::Duration::milliseconds(1200),
        ));
        clock_tick(&ui, &s, &tx);
        assert!(rx.try_recv().is_err(), "same minute is not resent");
        s.borrow_mut().clock_shown = None;
        clock_tick(&ui, &s, &tx);
        assert!(matches!(rx.try_recv().unwrap(), Job::RefreshClock { .. }));
        handle_event(&ui, &s, &tx, reply(Some("USB"))).unwrap();
        assert!(ui.get_layout_save_failed());
        s.borrow_mut().clock_shown = None;
        clock_tick(&ui, &s, &tx);
        assert!(rx.try_recv().is_err(), "paused after an error");
        // Other screen modes and layouts without a clock never tick.
        s.borrow_mut().clock_paused = false;
        s.borrow_mut().screen_layout.center.kind = Content::Empty;
        clock_tick(&ui, &s, &tx);
        assert!(rx.try_recv().is_err());
    }
}
