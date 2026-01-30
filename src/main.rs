use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use gstreamer as gst;
use gst::prelude::*;
use gst::bus::BusWatchGuard;

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use std::time::{Duration, Instant};

mod config;

#[derive(Clone)]
struct Camera {
    id: String,
    label: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct FrigateEvent {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    after: FrigateAfter,
}

#[derive(Debug, Default, Deserialize)]
struct FrigateAfter {
    #[serde(default)]
    camera: String,
    #[serde(default)]
    label: String,
}

#[derive(Debug, Clone)]
enum UiCommand {
    Event {
        camera_id: String,
        label: String,
    },
}



fn main() -> glib::ExitCode {
    gst::init().expect("Failed to initialize GStreamer");

    let config = config::load_config();

    let mut keys: Vec<&String> = config.cameras.keys().collect();
    keys.sort();

    let cameras: Vec<Camera> = keys
        .into_iter()
        .map(|key| {
            let camera = config
                .cameras
                .get(key)
                .expect("Camera key missing unexpectedly");
            Camera {
                id: key.clone(),
                label: camera
                    .label
                    .clone()
                    .unwrap_or_else(|| default_label(key)),
                url: camera.url.clone(),
            }
        })
        .collect();

    if cameras.is_empty() {
        panic!("No cameras defined in config.toml");
    }

    let app = adw::Application::builder()
        .application_id("com.garrett.gcamview")
        .build();

    app.connect_startup(|_| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferDark);
    });

    let mqtt = config.mqtt.clone();
    let actions = config.actions.clone();
    app.connect_activate(move |app| build_ui(app, cameras.clone(), mqtt.clone(), actions.clone()));

    app.run()
}

fn build_ui(
    app: &adw::Application,
    cameras: Vec<Camera>,
    mqtt: config::MqttConfig,
    actions: config::ActionsConfig,
) {
    const CSS: &str = r#"
    .camera-button {
        background-color: rgba(20, 20, 20, 0.9);
        color: #f7f7f7;
        min-height: 52px;
        min-width: 120px;
        padding: 8px 14px;
        font-size: 16px;
        border-radius: 14px;
        border: 1px solid rgba(255, 255, 255, 0.15);
    }
    .camera-button:checked {
        background-color: rgba(35, 35, 35, 0.95);
    }
    .audio-button {
        background-color: rgba(120, 20, 20, 0.9);
        color: #fff5f5;
        min-height: 46px;
        min-width: 140px;
        padding: 6px 14px;
        font-size: 15px;
        border-radius: 12px;
        border: 1px solid rgba(255, 255, 255, 0.25);
    }
    .audio-button:checked {
        background-color: rgba(20, 120, 20, 0.95);
        color: #f4fff4;
    }
    "#;

    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let picture = gtk::Picture::new();
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_can_shrink(true);

    let (first_label, first_url) = cameras
        .first()
        .expect("No cameras defined in config.toml")
        .clone()
        .into_label_url();

    let overlay_label = gtk::Label::new(Some(&format!("Loading {first_label}...")));
    overlay_label.set_margin_top(12);
    overlay_label.set_margin_start(12);
    overlay_label.set_halign(gtk::Align::Start);
    overlay_label.set_valign(gtk::Align::Start);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&picture));
    overlay.add_overlay(&overlay_label);

    let audio_enabled = Rc::new(RefCell::new(false));
    let current_url = Rc::new(RefCell::new(first_url.clone()));
    let current_label = Rc::new(RefCell::new(first_label.clone()));

    let (pipeline, paintable) = build_pipeline(&first_url, *audio_enabled.borrow());
    let pipeline = Rc::new(RefCell::new(pipeline));
    let bus_watch_guard: Rc<RefCell<Option<BusWatchGuard>>> = Rc::new(RefCell::new(None));
    picture.set_paintable(Some(&paintable));

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.set_halign(gtk::Align::Center);
    controls.set_valign(gtk::Align::Start);
    controls.set_margin_top(12);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    controls.set_hexpand(true);

    let mut first_button: Option<gtk::ToggleButton> = None;
    let mut button_by_id: HashMap<String, gtk::ToggleButton> = HashMap::new();
    let mut label_by_id: HashMap<String, String> = HashMap::new();
    for camera in cameras.iter() {
        let button = gtk::ToggleButton::with_label(&camera.label);
        button.add_css_class("camera-button");

        if let Some(existing) = &first_button {
            button.set_group(Some(existing));
        } else {
            button.set_active(true);
            first_button = Some(button.clone());
        }

        let pipeline_for_button = pipeline.clone();
        let bus_watch_for_button = bus_watch_guard.clone();
        let picture_for_button = picture.clone();
        let overlay_label_for_button = overlay_label.clone();
        let label = camera.label.clone();
        let url = camera.url.clone();
        let audio_enabled_for_button = audio_enabled.clone();
        let current_url_for_button = current_url.clone();
        let current_label_for_button = current_label.clone();

        button.connect_toggled(move |btn| {
            if btn.is_active() {
                *current_url_for_button.borrow_mut() = url.clone();
                *current_label_for_button.borrow_mut() = label.clone();
                overlay_label_for_button.set_text(&format!("Loading {label}..."));
                overlay_label_for_button.set_visible(true);

                let _ = bus_watch_for_button.borrow_mut().take();

                {
                    let current = pipeline_for_button.borrow_mut();
                    let _ = current.set_state(gst::State::Null);
                }

                let (new_pipeline, new_paintable) =
                    build_pipeline(&url, *audio_enabled_for_button.borrow());
                picture_for_button.set_paintable(Some(&new_paintable));

                {
                    let mut current = pipeline_for_button.borrow_mut();
                    *current = new_pipeline;
                }

                let guard = attach_bus_watch(
                    &pipeline_for_button.borrow(),
                    &overlay_label_for_button,
                );
                *bus_watch_for_button.borrow_mut() = Some(guard);

                pipeline_for_button
                    .borrow()
                    .set_state(gst::State::Playing)
                    .expect("Failed to set Playing");
            }
        });

        button_by_id.insert(camera.id.clone(), button.clone());
        label_by_id.insert(camera.id.clone(), camera.label.clone());
        controls.append(&button);
    }

    let audio_toggle = gtk::ToggleButton::with_label("Audio: Muted");
    audio_toggle.add_css_class("audio-button");
    audio_toggle.set_halign(gtk::Align::End);
    let audio_enabled_for_toggle = audio_enabled.clone();
    let pipeline_for_toggle = pipeline.clone();
    let bus_watch_for_toggle = bus_watch_guard.clone();
    let picture_for_toggle = picture.clone();
    let overlay_label_for_toggle = overlay_label.clone();
    let current_url_for_toggle = current_url.clone();
    let current_label_for_toggle = current_label.clone();
    audio_toggle.connect_toggled(move |btn| {
        *audio_enabled_for_toggle.borrow_mut() = btn.is_active();
        if btn.is_active() {
            btn.set_label("Audio: On");
        } else {
            btn.set_label("Audio: Muted");
        }
        let label = current_label_for_toggle.borrow().clone();
        let url = current_url_for_toggle.borrow().clone();
        overlay_label_for_toggle.set_text(&format!("Loading {label}..."));
        overlay_label_for_toggle.set_visible(true);

        let _ = bus_watch_for_toggle.borrow_mut().take();

        {
            let current = pipeline_for_toggle.borrow_mut();
            let _ = current.set_state(gst::State::Null);
        }

        let (new_pipeline, new_paintable) =
            build_pipeline(&url, *audio_enabled_for_toggle.borrow());
        picture_for_toggle.set_paintable(Some(&new_paintable));

        {
            let mut current = pipeline_for_toggle.borrow_mut();
            *current = new_pipeline;
        }

        let guard = attach_bus_watch(&pipeline_for_toggle.borrow(), &overlay_label_for_toggle);
        *bus_watch_for_toggle.borrow_mut() = Some(guard);

        pipeline_for_toggle
            .borrow()
            .set_state(gst::State::Playing)
            .expect("Failed to set Playing");
    });
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    controls.append(&spacer);
    controls.append(&audio_toggle);

    overlay.add_overlay(&controls);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("gcamview")
        .content(&overlay)
        .build();
    window.fullscreen();

    // esc to quit
    install_quit_shortcut(app, &window);

    let guard = attach_bus_watch(&pipeline.borrow(), &overlay_label);
    *bus_watch_guard.borrow_mut() = Some(guard);

    pipeline
        .borrow()
        .set_state(gst::State::Playing)
        .expect("Failed to set Playing");

    let (sender, receiver) = mpsc::channel::<UiCommand>();
    start_mqtt_thread(sender, mqtt);

    let button_by_id = Rc::new(button_by_id);
    let label_by_id = Rc::new(label_by_id);
    let actions = Rc::new(actions);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        for cmd in receiver.try_iter() {
            match cmd {
                UiCommand::Event { camera_id, label } => {
                    if let Some(button) = button_by_id.get(&camera_id) {
                        button.set_active(true);
                    } else {
                        eprintln!("MQTT switch requested unknown camera id: {camera_id}");
                    }

                    let camera_label = label_by_id
                        .get(&camera_id)
                        .cloned()
                        .unwrap_or_else(|| camera_id.clone());
                    let actions = actions.as_ref().clone();
                    thread::spawn(move || {
                        run_actions(&actions, &camera_id, &camera_label, &label);
                    });
                }
            }
        }
        glib::ControlFlow::Continue
    });

    let pipeline_for_close = pipeline.clone();
    window.connect_close_request(move |_| {
        let _ = pipeline_for_close.borrow().set_state(gst::State::Null);
        glib::Propagation::Proceed
    });

    window.present();
}

fn default_label(id: &str) -> String {
    let trimmed = id
        .strip_suffix("_rtsp_url")
        .or_else(|| id.strip_suffix("-rtsp-url"))
        .unwrap_or(id);
    let parts: Vec<&str> = trimmed
        .split(|c| c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return trimmed.to_string();
    }

    let mut label_parts = Vec::with_capacity(parts.len());
    for part in parts {
        let mut chars = part.chars();
        let mut label_part = String::new();
        if let Some(first) = chars.next() {
            label_part.push(first.to_ascii_uppercase());
            label_part.push_str(chars.as_str());
        }
        label_parts.push(label_part);
    }

    label_parts.join(" ")
}

impl Camera {
    fn into_label_url(self) -> (String, String) {
        (self.label, self.url)
    }
}

fn build_pipeline(uri: &str, audio_enabled: bool) -> (gst::Pipeline, gdk::Paintable) {
    let safe_uri = uri.replace('"', "%22");
    let audio_branch = if audio_enabled {
        r#"
            src. ! application/x-rtp,media=audio !
                queue ! decodebin ! audioconvert ! audioresample !
                autoaudiosink
        "#
    } else {
        ""
    };

    let desc = format!(
        r#"
        rtspsrc name=src location="{safe_uri}" protocols=tcp latency=100 drop-on-latency=true
            src. ! application/x-rtp,media=video !
                queue ! decodebin ! videoconvert !
                gtk4paintablesink name=sink sync=false
        {audio_branch}
        "#
    );

    let element = gst::parse::launch(&desc).expect("Failed to parse GStreamer pipeline");
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .expect("parse_launch did not return a gst::Pipeline");

    let sink = pipeline
        .by_name("sink")
        .expect("Pipeline missing element named 'sink' (gtk4paintablesink)");

    let paintable = sink.property::<gdk::Paintable>("paintable");
    (pipeline, paintable)
}

fn attach_bus_watch(
    pipeline: &gst::Pipeline,
    overlay_label: &gtk::Label,
) -> BusWatchGuard {
    let bus = pipeline
        .bus()
        .expect("Pipeline has no bus; cannot watch state");
    let overlay_label = overlay_label.clone();
    bus.add_watch_local(move |_, msg| {
        if let gst::MessageView::StateChanged(state) = msg.view() {
            if state.current() == gst::State::Playing {
                overlay_label.set_visible(false);
            }
        }
        glib::ControlFlow::Continue
    })
    .expect("Failed to add GStreamer bus watch")
}

fn install_quit_shortcut(app: &adw::Application, window: &adw::ApplicationWindow) {
    let quit_action = gio::SimpleAction::new("quit", None);
    let app_clone = app.clone();
    quit_action.connect_activate(move |_, _| {
        app_clone.quit();
    });
    app.add_action(&quit_action);

    let controller = gtk::ShortcutController::new();
    let trigger = gtk::KeyvalTrigger::new(gdk::Key::Escape, gdk::ModifierType::empty());
    let action = gtk::NamedAction::new("app.quit");
    let shortcut = gtk::Shortcut::new(Some(trigger), Some(action));
    controller.add_shortcut(shortcut);
    window.add_controller(controller);
}

fn start_mqtt_thread(sender: mpsc::Sender<UiCommand>, mqtt: config::MqttConfig) {
    std::thread::spawn(move || {
        eprintln!("MQTT: connecting to {}:{}", mqtt.host, mqtt.port);
        let client_id = format!("gcamview-{}", std::process::id());
        let mut opts = MqttOptions::new(client_id, mqtt.host, mqtt.port);
        opts.set_keep_alive(Duration::from_secs(30));

        // If you require username/password, uncomment:
        // if let (Ok(u), Ok(p)) = (std::env::var("MQTT_USER"), std::env::var("MQTT_PASS")) {
        //     opts.set_credentials(u, p);
        // }

        let (client, mut connection) = Client::new(opts, 10);

        if let Err(err) = client.subscribe("frigate/events", QoS::AtMostOnce) {
            eprintln!("MQTT: failed to subscribe to frigate/events: {err}");
            return;
        }
        eprintln!("MQTT: subscribed to frigate/events");

        // Basic throttle to avoid rapid-fire switches
        let mut last_switch = Instant::now() - Duration::from_secs(60);
        let mut last_camera: Option<String> = None;
        let min_gap = Duration::from_millis(600);

        for notification in connection.iter() {
            let Ok(Event::Incoming(Packet::Publish(p))) = notification else {
                continue;
            };
            let payload = String::from_utf8_lossy(&p.payload);
            eprintln!("MQTT: message topic={} payload={}", p.topic, payload);

            // Parse the JSON payload
            let evt: Result<FrigateEvent, _> = serde_json::from_slice(&p.payload);
            let Ok(evt) = evt else {
                eprintln!("MQTT: failed to parse event payload");
                continue;
            };

            // Only respond to "new" events and people (Frigate publishes new/update/end)
            if evt.r#type != "new" {
                eprintln!("MQTT: ignoring event type={}", evt.r#type);
                continue;
            }

            let cam = evt.after.camera;
            let label = evt.after.label;
            if cam.is_empty() {
                eprintln!("MQTT: event missing camera name");
                continue;
            }
            if label != "person" {
                eprintln!("MQTT: ignoring non-person label={label}");
                continue;
            }

            if last_camera.as_deref() == Some(cam.as_str()) && last_switch.elapsed() < min_gap {
                continue;
            }
            last_switch = Instant::now();
            last_camera = Some(cam.clone());

            if !label.is_empty() {
                eprintln!("Frigate event: camera={cam} label={label}");
            }
            let _ = sender.send(UiCommand::Event {
                camera_id: cam,
                label,
            });
        }
    });
}

fn run_actions(
    actions: &config::ActionsConfig,
    camera_id: &str,
    camera_label: &str,
    label: &str,
) {
    if actions.wake.enabled {
        for cmd in actions.wake.commands.iter() {
            run_shell_command(cmd);
        }
    }

    if actions.tts.enabled {
        let spoken = tts_phrase(camera_id, camera_label, label);
        let escaped = shell_escape_single_quotes(&spoken);
        let cmd = actions
            .tts
            .command
            .replace("{text}", &escaped);
        run_shell_command(&cmd);
    }
}

fn run_shell_command(cmd: &str) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status();
    if let Err(err) = status {
        eprintln!("Action command failed: {cmd} ({err})");
    }
}

fn shell_escape_single_quotes(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn tts_phrase(camera_id: &str, camera_label: &str, label: &str) -> String {
    let label = label.to_ascii_lowercase();
    if label == "person" {
        match camera_id {
            "driveway" => "Person in driveway".to_string(),
            "stairs" => "Person on stairs".to_string(),
            "front_door" => "Person at front door".to_string(),
            _ => format!("Person at {camera_label}"),
        }
    } else {
        format!("{} at {camera_label}", capitalize_first(&label))
    }
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::new();
    out.push(first.to_ascii_uppercase());
    out.push_str(chars.as_str());
    out
}
