//! Anchor — turn any website into a real desktop app.

mod browser;
mod desktop;
mod favicon;
mod paths;
mod ui;
mod webapp;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use libadwaita::Application;

use crate::paths::APP_ID;
use crate::ui::MainWindow;

fn main() -> glib::ExitCode {
    // Ensure data directories exist early
    if let Err(e) = paths::ensure_dirs() {
        eprintln!("warning: could not create data dirs: {e}");
    }
    // Fix StartupWMClass for Chromium Wayland dock icons on existing apps
    match webapp::repair_all_webapps() {
        Ok(0) => {}
        Ok(n) => eprintln!("repaired dock matching for {n} web app(s)"),
        Err(e) => eprintln!("warning: could not repair web apps: {e}"),
    }

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        // Single main window
        let existing = app
            .windows()
            .into_iter()
            .find_map(|w| w.downcast::<libadwaita::ApplicationWindow>().ok());

        if let Some(win) = existing {
            win.present();
            return;
        }

        let main = MainWindow::new(app);
        install_refresh_action(app, &main);
        main.present();
    });

    app.run()
}

fn install_refresh_action(app: &Application, main: &std::rc::Rc<MainWindow>) {
    let action = gio::SimpleAction::new("refresh", None);
    let main = std::rc::Rc::clone(main);
    action.connect_activate(move |_, _| {
        main.reload();
    });
    app.add_action(&action);
}
