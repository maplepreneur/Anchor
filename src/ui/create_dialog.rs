//! Dialog to create or edit a web app (name, URL, browser, profile, icon).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, FileDialog, FileFilter, Image, Label, Orientation, PolicyType,
    ScrolledWindow, StringList,
};
use libadwaita::prelude::*;
use libadwaita::{ComboRow, EntryRow, PreferencesGroup, SwitchRow};

use crate::browser::{self, ProfileMode};
use crate::desktop::DesktopEntry;
use crate::webapp::{self, CreateRequest, EditRequest, IconSource};

enum IconState {
    /// No icon chosen yet (create), or will re-fetch if needed.
    None,
    /// Keep the icon already installed for this app (edit).
    Keep,
    Preview(PathBuf),
    Local(PathBuf),
}

pub struct CreateDialog;

impl CreateDialog {
    pub fn show<F>(parent: &impl IsA<gtk::Widget>, on_done: F)
    where
        F: Fn(anyhow::Result<DesktopEntry>) + 'static,
    {
        Self::show_inner(parent, None, on_done);
    }

    pub fn show_edit<F>(parent: &impl IsA<gtk::Widget>, existing: DesktopEntry, on_done: F)
    where
        F: Fn(anyhow::Result<DesktopEntry>) + 'static,
    {
        Self::show_inner(parent, Some(existing), on_done);
    }

    fn show_inner<F>(
        parent: &impl IsA<gtk::Widget>,
        existing: Option<DesktopEntry>,
        on_done: F,
    ) where
        F: Fn(anyhow::Result<DesktopEntry>) + 'static,
    {
        let is_edit = existing.is_some();
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

        // Floating + fixed height so the dialog is not clipped to a short parent
        // window; the form body scrolls when content is taller.
        let dialog = libadwaita::Dialog::builder()
            .title(if is_edit {
                "Edit Web App"
            } else {
                "Add Web App"
            })
            .content_width(500)
            .content_height(620)
            .presentation_mode(libadwaita::DialogPresentationMode::Floating)
            .build();

        let toast_overlay = libadwaita::ToastOverlay::new();

        // EntryRow / ComboRow participate correctly in keyboard focus (Tab order).
        let name_row = EntryRow::builder()
            .title("Name")
            .text(
                existing
                    .as_ref()
                    .map(|a| a.name.as_str())
                    .unwrap_or(""),
            )
            .build();
        name_row.set_show_apply_button(false);

        let url_row = EntryRow::builder()
            .title("URL")
            .text(existing.as_ref().map(|a| a.url.as_str()).unwrap_or(""))
            .build();
        url_row.set_show_apply_button(false);

        // Browser dropdown (ComboRow)
        let browser_names: Vec<String> = browsers.iter().map(|b| b.name.clone()).collect();
        let browser_model =
            StringList::new(&browser_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut browser_selected: u32 = 0;
        if let Some(app) = existing.as_ref() {
            if let Some(idx) = browsers.iter().position(|b| b.name == app.browser) {
                browser_selected = idx as u32;
            } else if let Some(idx) = browsers.iter().position(|b| {
                app.browser
                    .to_ascii_lowercase()
                    .contains(&b.name.to_ascii_lowercase())
                    || b.name
                        .to_ascii_lowercase()
                        .contains(&app.browser.to_ascii_lowercase())
            }) {
                browser_selected = idx as u32;
            }
        }
        let browser_row = ComboRow::builder()
            .title("Browser")
            .model(&browser_model)
            .selected(browser_selected)
            .build();
        // Display StringObject.string for each item
        browser_row.set_expression(Some(
            gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                gtk::Expression::NONE,
                "string",
            )
            .upcast(),
        ));

        // Profile mode
        let profile_labels = ["Isolated", "Shared browser profile"];
        let profile_model = StringList::new(&profile_labels);
        let initial_profile = existing
            .as_ref()
            .map(|a| a.profile_mode)
            .unwrap_or(ProfileMode::Isolated);
        let profile_row = ComboRow::builder()
            .title("Profile")
            .subtitle(profile_mode_subtitle(initial_profile))
            .model(&profile_model)
            .selected(profile_mode_to_index(initial_profile))
            .build();
        profile_row.set_expression(Some(
            gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                gtk::Expression::NONE,
                "string",
            )
            .upcast(),
        ));

        // Title bar: off by default for a frameless app window (matches X.com-style).
        let initial_show_title = existing
            .as_ref()
            .map(|a| a.show_title_bar)
            .unwrap_or(false);
        let title_bar_row = SwitchRow::builder()
            .title("Show title bar")
            .subtitle(title_bar_subtitle(initial_show_title))
            .active(initial_show_title)
            .build();
        title_bar_row.connect_active_notify(|row| {
            row.set_subtitle(title_bar_subtitle(row.is_active()));
        });

        let form = PreferencesGroup::builder()
            .title("Web App")
            .description(if is_edit {
                "Update this app’s name, URL, browser, profile, window chrome, or icon. Tab moves between fields."
            } else {
                "Choose browser profile, window chrome, and icon. Tab moves between fields."
            })
            .build();
        form.add(&name_row);
        form.add(&url_row);
        form.add(&browser_row);
        form.add(&profile_row);
        form.add(&title_bar_row);

        // Icon section
        let icon_image = Image::from_icon_name("image-missing-symbolic");
        icon_image.set_pixel_size(64);

        let icon_status = Label::builder()
            .label(if is_edit {
                "Current icon (fetch or choose to replace)"
            } else {
                "Icon will be fetched from the website"
            })
            .css_classes(["dim-label", "caption"])
            .halign(Align::Start)
            .wrap(true)
            .build();

        let mut initial_icon = IconState::None;
        if let Some(app) = existing.as_ref() {
            let path = webapp::resolve_icon_file(&app.codename, &app.icon);
            if path.is_file() {
                icon_image.set_from_file(Some(&path));
                icon_status.set_label("Current icon");
                initial_icon = IconState::Keep;
            }
        }

        let fetch_btn = Button::builder()
            .label("Fetch icon")
            .focusable(true)
            .build();
        let choose_btn = Button::builder()
            .label("Choose image…")
            .focusable(true)
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
        let page = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_top(18)
            .margin_bottom(12)
            .margin_start(18)
            .margin_end(18)
            .build();
        page.append(&form);
        page.append(&icon_group);
        page.append(&icon_row);

        let hint = Label::builder()
            .label(profile_mode_hint(initial_profile))
            .css_classes(["dim-label", "caption"])
            .wrap(true)
            .halign(Align::Start)
            .build();
        page.append(&hint);

        {
            let hint = hint.clone();
            profile_row.connect_selected_notify(move |row| {
                let mode = profile_mode_from_index(row.selected());
                hint.set_label(profile_mode_hint(mode));
                row.set_subtitle(profile_mode_subtitle(mode));
            });
        }

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .propagate_natural_width(true)
            .vexpand(true)
            .hexpand(true)
            .child(&page)
            .build();

        let submit_btn = Button::builder()
            .label(if is_edit { "Save" } else { "Create" })
            .css_classes(["suggested-action", "pill"])
            .halign(Align::End)
            .focusable(true)
            .sensitive(is_edit)
            .build();
        let cancel_btn = Button::builder()
            .label("Cancel")
            .css_classes(["pill"])
            .halign(Align::End)
            .focusable(true)
            .build();

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(Align::End)
            .build();
        actions.append(&cancel_btn);
        actions.append(&submit_btn);

        let shortcut_hint = Label::builder()
            .label("Tab / Shift+Tab move fields · Enter creates/saves · Esc cancels")
            .css_classes(["dim-label", "caption"])
            .halign(Align::Start)
            .wrap(true)
            .build();

        let footer = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_top(8)
            .margin_bottom(16)
            .margin_start(18)
            .margin_end(18)
            .build();
        footer.append(&shortcut_hint);
        footer.append(&actions);

        let outer = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .build();
        outer.append(&scrolled);
        outer.append(&footer);

        toast_overlay.set_child(Some(&outer));
        dialog.set_child(Some(&toast_overlay));

        let icon_state = Rc::new(RefCell::new(initial_icon));
        let browsers = Rc::new(browsers);
        let existing = Rc::new(existing);

        let update_sensitive = {
            let name_row = name_row.clone();
            let url_row = url_row.clone();
            let submit_btn = submit_btn.clone();
            move || {
                let name_ok = !name_row.text().trim().is_empty();
                let url_ok = !url_row.text().trim().is_empty();
                submit_btn.set_sensitive(name_ok && url_ok);
            }
        };

        {
            let update = update_sensitive.clone();
            name_row.connect_changed(move |_: &EntryRow| update());
        }
        {
            let update = update_sensitive.clone();
            url_row.connect_changed(move |_: &EntryRow| update());
        }
        update_sensitive();

        // Fetch icon
        {
            let url_row = url_row.clone();
            let icon_image = icon_image.clone();
            let icon_status = icon_status.clone();
            let icon_state = Rc::clone(&icon_state);
            let toast_overlay = toast_overlay.clone();
            let update = update_sensitive.clone();
            let fetch_btn_c = fetch_btn.clone();

            fetch_btn.connect_clicked(move |_| {
                let url = url_row.text().to_string();
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

        // Enter in URL (create): fetch icon if needed, else submit is handled below
        if !is_edit {
            let fetch_btn = fetch_btn.clone();
            let icon_state = Rc::clone(&icon_state);
            url_row.connect_entry_activated(move |_| {
                if matches!(*icon_state.borrow(), IconState::None) {
                    fetch_btn.emit_clicked();
                }
            });
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

                let parent_win = dialog_w.root().and_downcast::<gtk::Window>();

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

        // Shared submit logic for button + Enter on name
        let do_submit = {
            let dialog = dialog.clone();
            let name_row = name_row.clone();
            let url_row = url_row.clone();
            let browser_row = browser_row.clone();
            let profile_row = profile_row.clone();
            let title_bar_row = title_bar_row.clone();
            let browsers = Rc::clone(&browsers);
            let icon_state = Rc::clone(&icon_state);
            let toast_overlay = toast_overlay.clone();
            let submit_btn = submit_btn.clone();
            let existing = Rc::clone(&existing);
            let on_done = Rc::new(RefCell::new(Some(on_done)));
            let icon_image = icon_image.clone();
            let icon_status = icon_status.clone();
            let is_edit = is_edit;

            Rc::new(move || {
                if !submit_btn.is_sensitive() {
                    return;
                }
                let name = name_row.text().to_string();
                let url = url_row.text().to_string();
                let idx = browser_row.selected() as usize;
                let Some(browser) = browsers.get(idx).cloned() else {
                    toast_overlay.add_toast(libadwaita::Toast::new("Select a browser"));
                    return;
                };
                let profile_mode = profile_mode_from_index(profile_row.selected());
                let show_title_bar = title_bar_row.is_active();

                let icon_source = match &*icon_state.borrow() {
                    IconState::None => IconSource::Fetch,
                    IconState::Keep => IconSource::KeepExisting,
                    IconState::Preview(p) => IconSource::PreparedPng(p.clone()),
                    IconState::Local(p) => IconSource::Local(p.clone()),
                };

                submit_btn.set_sensitive(false);
                let result = if let Some(app) = existing.as_ref() {
                    webapp::update_webapp(EditRequest {
                        existing: app.clone(),
                        name,
                        url,
                        browser,
                        icon_source,
                        profile_mode,
                        show_title_bar,
                    })
                } else {
                    webapp::create_webapp(CreateRequest {
                        name,
                        url,
                        browser,
                        icon_override: None,
                        icon_source,
                        profile_mode,
                        show_title_bar,
                    })
                };

                match result {
                    Ok(entry) => {
                        dialog.close();
                        if let Some(cb) = on_done.borrow_mut().take() {
                            cb(Ok(entry));
                        }
                    }
                    Err(e) => {
                        submit_btn.set_sensitive(true);
                        let msg = format!("{e:#}");
                        if msg.contains("favicon") || msg.contains("icon") {
                            icon_image.set_icon_name(Some("dialog-warning-symbolic"));
                            icon_status.set_label(if is_edit {
                                "Could not update icon. Choose an image file, then Save again."
                            } else {
                                "Could not fetch favicon. Choose an image file, then Create again."
                            });
                            if !is_edit {
                                *icon_state.borrow_mut() = IconState::None;
                            }
                        }
                        toast_overlay.add_toast(libadwaita::Toast::new(&msg));
                    }
                }
            })
        };

        {
            let do_submit = Rc::clone(&do_submit);
            submit_btn.connect_clicked(move |_| do_submit());
        }
        // Enter in Name field submits when form is valid
        {
            let do_submit = Rc::clone(&do_submit);
            name_row.connect_entry_activated(move |_| do_submit());
        }
        // Enter in URL (edit, or after icon ready) submits
        {
            let do_submit = Rc::clone(&do_submit);
            let icon_state = Rc::clone(&icon_state);
            let is_edit = is_edit;
            url_row.connect_entry_activated(move |_| {
                if is_edit || !matches!(*icon_state.borrow(), IconState::None) {
                    do_submit();
                }
            });
        }

        dialog.present(Some(parent));

        // Focus the name field so the first Tab starts from a predictable place
        let name_focus = name_row.clone();
        glib::idle_add_local_once(move || {
            let _ = name_focus.grab_focus();
        });
    }
}

fn profile_mode_from_index(idx: u32) -> ProfileMode {
    match idx {
        1 => ProfileMode::Shared,
        _ => ProfileMode::Isolated,
    }
}

fn profile_mode_to_index(mode: ProfileMode) -> u32 {
    match mode {
        ProfileMode::Isolated => 0,
        ProfileMode::Shared => 1,
    }
}

fn profile_mode_subtitle(mode: ProfileMode) -> &'static str {
    match mode {
        ProfileMode::Isolated => "Separate empty profile — independent of your main browser",
        ProfileMode::Shared => {
            "Private profile, seeded with your browser’s logins, site data & extensions"
        }
    }
}

fn profile_mode_hint(mode: ProfileMode) -> &'static str {
    match mode {
        ProfileMode::Isolated => {
            "Note: Starts signed out with its own dock icon. Sign in once inside the web app."
        }
        ProfileMode::Shared => {
            "Note: Copies extensions, cookies, and site storage (IndexedDB/localStorage — needed for WhatsApp, etc.) from your browser into a private profile so the web app keeps its own dock icon. Close the browser and the web app first for a complete copy. Re-seed by switching away from Shared and back again."
        }
    }
}

fn title_bar_subtitle(show: bool) -> &'static str {
    if show {
        "Native window title bar with close / minimize / maximize (Firefox)"
    } else {
        "Frameless window — hide the title bar for an app-like look (Firefox; default)"
    }
}

/// Run blocking favicon fetch off the GTK thread, then callback on the main loop.
fn gio_spawn_fetch<F>(url: String, on_done: F)
where
    F: FnOnce(anyhow::Result<PathBuf>) + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();

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
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(cb) = on_done.take() {
                    cb(Err(anyhow::anyhow!("favicon worker closed")));
                }
                glib::ControlFlow::Break
            }
        }
    });
}
