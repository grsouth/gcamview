use adw::prelude::*;
use gtk::{gdk, gio, glib};

fn main() {
    let app = adw::Application::builder()
        .application_id("com.garrett.gcamview")
        .build();

    app.connect_activate(build_ui);

    app.run();
}

fn build_ui(app: &adw::Application) {

    let label = gtk::Label::new(Some("Hello, gcamview!"));
    label.set_margin_top(20);
    label.set_margin_bottom(20);
    label.set_margin_start(20);
    label.set_margin_end(20);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("gcamview")
        .content(&label)
        .build();
    window.fullscreen();

    window.present();
}