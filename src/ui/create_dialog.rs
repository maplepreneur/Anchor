//! Dialog to create a new web app (name, URL, browser, icon).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, DropDown, Entry, FileDialog, FileFilter, Image, Label,
    Orientation, StringList,
};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, PreferencesGroup};

use crate::browser;
use crate::desktop::DesktopEntry;
use crate::webapp::{self, CreateRequest, IconSource};

enum IconState {
    None,
    Preview(PathBuf),
    Local(PathBuf),
}

pub struct CreateDialog;

impl CreateDialog {
    pub fn show<F>(parent: &impl IsA<gtk::Widget>, on_done: F)
    where
        F: Fn(anyhow::Result<DesktopEntry>) + 'static,
    {
        let browsers = browser::detect_browsers();
        if browsers.is_empty() {
            let alert = libadwaita::AlertDialog::builder()
                .heading("No browsers found")
                .body("Install Firefox, Brave, Chrome, or Chromium, then try again.")
                .build();
            alert.add_response("ok", "OK");
            alert.present(Some(parent));
            return;
        }

        let dialog = libadwaita::Dialog::builder()
            .title("Add Web App")
            .content_width(480)
            .build();

        let toast_overlay = libadwaita::ToastOverlay::new();

        let name_entry = Entry::builder()
            .placeholder_text("e.g. YouTube")
            .hexpand(true)
            .build();
        let url_entry = Entry::builder()
            .placeholder_text("e.g. https://youtube.com")
            .hexpand(true)
            .build();

        let name_row = ActionRow::builder().title("Name").build();
        name_row.add_suffix(&name_entry);
        name_row.set_activatable_widget(Some(&name_entry));

        let url_row = ActionRow::builder().title("URL").build();
        url_row.add_suffix(&url_entry);
        url_row.set_activatable_widget(Some(&url_entry));

        // Browser dropdown
        let browser_names: Vec<String> = browsers.iter().map(|b| b.name.clone()).collect();
        let model = StringList::new(&browser_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let browser_drop = DropDown::builder().model(&model).selected(0).build();
        let browser_row = ActionRow::builder().title("Browser").build();
        browser_row.add_suffix(&browser_drop);

        let form = PreferencesGroup::builder()
            .title("Web App")
            .description("Isolated apps use a separate browser profile so they stay independent of your main browser.")
            .build();
        form.add(&name_row);
        form.add(&url_row);
        form.add(&browser_row);

        // Icon section
        let icon_image = Image::from_icon_name("image-missing-symbolic");
        icon_image.set_pixel_size(64);

        let icon_status = Label::builder()
            .label("Icon will be fetched from the website")
            .css_classes(["dim-label", "caption"])
            .halign(Align::Start)
            .wrap(true)
            .build();

        let fetch_btn = Button::builder()
            .label("Fetch icon")
            .build();
        let choose_btn = Button::builder()
            .label("Choose image…")
            .build();

        let icon_buttons = GtkBox::new(Orientation::Horizontal, 8);
        icon_buttons.append(&fetch_btn);
        icon_buttons.append(&choose_btn);

        let icon_text = GtkBox::new(Orientation::Vertical, 6);
        icon_text.append(&icon_status);
        icon_text.append(&icon_buttons);
        icon_text.set_hexpand(true);
        icon_text.set_valign(Align::Center);

        let icon_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .margin_top(12)
            .build();
        icon_row.append(&icon_image);
        icon_row.append(&icon_text);

        let icon_group = PreferencesGroup::builder().title("Icon").build();
        // PreferencesGroup expects ActionRow-like; use a plain box inside a page instead
        let page = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        page.append(&form);
        page.append(&icon_group);
        page.append(&icon_row);

        let hint = Label::builder()
            .label("Note: Isolated profiles start signed out. Sign in once inside the web app.")
            .css_classes(["dim-label", "caption"])
            .wrap(true)
            .halign(Align::Start)
            .build();
        page.append(&hint);

        let create_btn = Button::builder()
            .label("Create")
            .css_classes(["suggested-action", "pill"])
            .halign(Align::End)
            .sensitive(false)
            .build();
        let cancel_btn = Button::builder()
            .label("Cancel")
            .css_classes(["pill"])
            .halign(Align::End)
            .build();

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(Align::End)
            .margin_top(12)
            .build();
        actions.append(&cancel_btn);
        actions.append(&create_btn);
        page.append(&actions);

        toast_overlay.set_child(Some(&page));
        dialog.set_child(Some(&toast_overlay));

        let icon_state = Rc::new(RefCell::new(IconState::None));
        let browsers = Rc::new(browsers);

        // Enable Create when name + URL are set (icon fetched on Create if missing)
        let update_sensitive = {
            let name_entry = name_entry.clone();
            let url_entry = url_entry.clone();
            let create_btn = create_btn.clone();
            move || {
                let name_ok = !name_entry.text().trim().is_empty();
                let url_ok = !url_entry.text().trim().is_empty();
                create_btn.set_sensitive(name_ok && url_ok);
            }
        };

        {
            let update = update_sensitive.clone();
            name_entry.connect_changed(move |_| update());
        }
        {
            let update = update_sensitive.clone();
            url_entry.connect_changed(move |_| update());
        }

        // Fetch icon
        {
            let url_entry = url_entry.clone();
            let icon_image = icon_image.clone();
            let icon_status = icon_status.clone();
            let icon_state = Rc::clone(&icon_state);
            let toast_overlay = toast_overlay.clone();
            let update = update_sensitive.clone();
            let fetch_btn_c = fetch_btn.clone();

            fetch_btn.connect_clicked(move |_| {
                let url = url_entry.text().to_string();
                if url.trim().is_empty() {
                    toast_overlay.add_toast(libadwaita::Toast::new("Enter a URL first"));
                    return;
                }
                fetch_btn_c.set_sensitive(false);
                icon_status.set_label("Fetching icon…");

                let icon_image = icon_image.clone();
                let icon_status = icon_status.clone();
                let icon_state = Rc::clone(&icon_state);
                let toast_overlay = toast_overlay.clone();
                let update = update.clone();
                let fetch_btn_c = fetch_btn_c.clone();

                // Background thread for network I/O
                gio_spawn_fetch(url, move |result| {
                    fetch_btn_c.set_sensitive(true);
                    match result {
                        Ok(path) => {
                            icon_image.set_from_file(Some(&path));
                            icon_status.set_label("Icon ready");
                            *icon_state.borrow_mut() = IconState::Preview(path);
                            update();
                        }
                        Err(e) => {
                            icon_image.set_icon_name(Some("dialog-warning-symbolic"));
                            icon_status.set_label(
                                "Could not fetch favicon. Choose an image file for the icon.",
                            );
                            toast_overlay.add_toast(libadwaita::Toast::new(&format!("{e:#}")));
                            update();
                        }
                    }
                });
            });
        }

        // Auto-fetch when URL loses focus (if no icon yet)
        {
            let url_entry_c = url_entry.clone();
            let fetch_btn = fetch_btn.clone();
            let icon_state = Rc::clone(&icon_state);
            url_entry.connect_activate(move |_| {
                if matches!(*icon_state.borrow(), IconState::None) {
                    fetch_btn.emit_clicked();
                }
            });
            let _ = url_entry_c;
        }

        // Choose local image
        {
            let icon_image = icon_image.clone();
            let icon_status = icon_status.clone();
            let icon_state = Rc::clone(&icon_state);
            let update = update_sensitive.clone();
            let dialog_w = dialog.clone();

            choose_btn.connect_clicked(move |_| {
                let filter = FileFilter::new();
                filter.set_name(Some("Images"));
                filter.add_mime_type("image/png");
                filter.add_mime_type("image/jpeg");
                filter.add_mime_type("image/webp");
                filter.add_mime_type("image/gif");
                filter.add_mime_type("image/x-icon");
                filter.add_mime_type("image/vnd.microsoft.icon");
                filter.add_pattern("*.png");
                filter.add_pattern("*.jpg");
                filter.add_pattern("*.jpeg");
                filter.add_pattern("*.webp");
                filter.add_pattern("*.ico");
                filter.add_pattern("*.gif");

                let filters = gtk::gio::ListStore::new::<FileFilter>();
                filters.append(&filter);

                let file_dialog = FileDialog::builder()
                    .title("Choose icon image")
                    .modal(true)
                    .filters(&filters)
                    .default_filter(&filter)
                    .build();

                let icon_image = icon_image.clone();
                let icon_status = icon_status.clone();
                let icon_state = Rc::clone(&icon_state);
                let update = update.clone();

                let parent_win = dialog_w
                    .root()
                    .and_downcast::<gtk::Window>();

                file_dialog.open(
                    parent_win.as_ref(),
                    gtk::gio::Cancellable::NONE,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                icon_image.set_from_file(Some(&path));
                                icon_status.set_label("Custom icon selected");
                                *icon_state.borrow_mut() = IconState::Local(path);
                                update();
                            }
                        }
                    },
                );
            });
        }

        // Cancel
        {
            let dialog = dialog.clone();
            cancel_btn.connect_clicked(move |_| {
                dialog.close();
            });
        }

        // Create
        {
            let dialog = dialog.clone();
            let name_entry = name_entry.clone();
            let url_entry = url_entry.clone();
            let browser_drop = browser_drop.clone();
            let browsers = Rc::clone(&browsers);
            let icon_state = Rc::clone(&icon_state);
            let toast_overlay = toast_overlay.clone();
            let create_btn = create_btn.clone();
            let on_done = Rc::new(RefCell::new(Some(on_done)));

            create_btn.connect_clicked(move |btn| {
                let name = name_entry.text().to_string();
                let url = url_entry.text().to_string();
                let idx = browser_drop.selected() as usize;
                let Some(browser) = browsers.get(idx).cloned() else {
                    toast_overlay.add_toast(libadwaita::Toast::new("Select a browser"));
                    return;
                };

                let icon_source = match &*icon_state.borrow() {
                    IconState::None => IconSource::Fetch,
                    IconState::Preview(p) => IconSource::PreparedPng(p.clone()),
                    IconState::Local(p) => IconSource::Local(p.clone()),
                };

                btn.set_sensitive(false);
                // May briefly block the UI while fetching a favicon over the network.
                match webapp::create_webapp(CreateRequest {
                    name,
                    url,
                    browser,
                    icon_override: None,
                    icon_source,
                }) {
                    Ok(entry) => {
                        dialog.close();
                        if let Some(cb) = on_done.borrow_mut().take() {
                            cb(Ok(entry));
                        }
                    }
                    Err(e) => {
                        btn.set_sensitive(true);
                        let msg = format!("{e:#}");
                        if msg.contains("favicon") || msg.contains("icon") {
                            icon_image.set_icon_name(Some("dialog-warning-symbolic"));
                            icon_status.set_label(
                                "Could not fetch favicon. Choose an image file, then Create again.",
                            );
                            *icon_state.borrow_mut() = IconState::None;
                        }
                        toast_overlay.add_toast(libadwaita::Toast::new(&msg));
                    }
                }
            });
        }

        dialog.present(Some(parent));
    }
}

/// Run blocking favicon fetch off the GTK thread, then callback on the main loop.
fn gio_spawn_fetch<F>(url: String, on_done: F)
where
    F: FnOnce(anyhow::Result<PathBuf>) + 'static,
{
    let (sender, receiver) = async_channel_unbounded();

    std::thread::spawn(move || {
        let result = webapp::preview_favicon(&url);
        let _ = sender.send(result);
    });

    let mut on_done = Some(on_done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(result) => {
                if let Some(cb) = on_done.take() {
                    cb(result);
                }
                glib::ControlFlow::Break
            }
            Err(async_channel::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(async_channel::TryRecvError::Closed) => {
                if let Some(cb) = on_done.take() {
                    cb(Err(anyhow::anyhow!("favicon worker closed")));
                }
                glib::ControlFlow::Break
            }
        }
    });
}

// Minimal unbounded channel without extra crate — use std mpsc + try_recv via RefCell
// Actually std::sync::mpsc is fine with try_recv
mod async_channel {
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError as StdTry};

    pub struct SenderWrap<T>(Sender<T>);
    pub struct ReceiverWrap<T>(Receiver<T>);

    pub enum TryRecvError {
        Empty,
        Closed,
    }

    impl<T> SenderWrap<T> {
        pub fn send(&self, v: T) -> Result<(), ()> {
            self.0.send(v).map_err(|_| ())
        }
    }

    impl<T> ReceiverWrap<T> {
        pub fn try_recv(&self) -> Result<T, TryRecvError> {
            match self.0.try_recv() {
                Ok(v) => Ok(v),
                Err(StdTry::Empty) => Err(TryRecvError::Empty),
                Err(StdTry::Disconnected) => Err(TryRecvError::Closed),
            }
        }
    }

    pub fn unbounded<T>() -> (SenderWrap<T>, ReceiverWrap<T>) {
        let (s, r) = mpsc::channel();
        (SenderWrap(s), ReceiverWrap(r))
    }
}

fn async_channel_unbounded<T>() -> (async_channel::SenderWrap<T>, async_channel::ReceiverWrap<T>) {
    async_channel::unbounded()
}
