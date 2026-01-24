use adw::prelude::*;
use gtk::{gdk, glib};

use gstreamer as gst;
use gst::prelude::*;

mod config;

fn main() -> glib::ExitCode {
    // initialize gstreamer
    gst::init().expect("Failed to initialize GStreamer");

    // Load secret config values (url with username/password)
    let config = config::load_config();

    let mut keys: Vec<&String> = config.camera_urls.keys().collect();
    keys.sort();

    let first_key: &str = keys
        .first()
        .expect("No cameras defined in config.toml")
        .as_str();

    let first_camera_url: String = config
        .camera_urls
        .get(first_key)
        .expect("Camera key missing unexpectedly")
        .clone();

    let app = adw::Application::builder()
        .application_id("com.garrett.gcamview")
        .build();

    // Move first_camera_url into the activate handler
    app.connect_activate(move |app| build_ui(app, first_camera_url.clone()));

    app.run()
}

fn build_ui(app: &adw::Application, first_camera_url: String) {
    let picture = gtk::Picture::new();
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_can_shrink(true);

    let overlay_label = gtk::Label::new(Some("Loading stream..."));
    overlay_label.set_margin_top(12);
    overlay_label.set_margin_start(12);
    overlay_label.set_halign(gtk::Align::Start);
    overlay_label.set_valign(gtk::Align::Start);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&picture));
    overlay.add_overlay(&overlay_label);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("gcamview")
        .content(&overlay)
        .build();
    window.fullscreen();

    let (playbin, paintable) = build_player(&first_camera_url);
    picture.set_paintable(Some(&paintable));

    playbin
        .set_state(gst::State::Playing)
        .expect("Failed to set Playing");

    let playbin_for_close = playbin.clone();
    window.connect_close_request(move |_| {
        let _ = playbin_for_close.set_state(gst::State::Null);
        glib::Propagation::Proceed
    });
    window.present();
}

fn build_player(uri: &str) -> (gst::Element, gdk::Paintable) {
    let sink = gst::ElementFactory::make("gtk4paintablesink")
        .build()
        .expect("Missing gtk4paintablesink. Install 'gst-plugin-gtk4' and verify with gst-inspect-1.0 gtk4paintablesink");

    let playbin = gst::ElementFactory::make("playbin3")
        .build()
        .or_else(|_| gst::ElementFactory::make("playbin").build())
        .expect("Missing playbin/playbin3 element");

    playbin.set_property("uri", uri);
    playbin.set_property("video-sink", &sink);

    let paintable = sink.property::<gdk::Paintable>("paintable");
    (playbin, paintable)
}
