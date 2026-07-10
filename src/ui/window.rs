//! Main application window: list web apps, add, delete, keyboard shortcuts.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk::{
    gio, glib, Align, Box as GtkBox, Button, EventControllerKey, Label, ListBox, ListBoxRow,
    Orientation, PolicyType, ScrolledWindow,
};
use libadwaita::prelude::*;
use libadwaita::{
    ActionRow, Application, ApplicationWindow, HeaderBar, PreferencesGroup, StatusPage, Toast,
    ToastOverlay, ToolbarView,
};

use crate::browser::ProfileMode;
use crate::desktop::DesktopEntry;
use crate::ui::create_dialog::CreateDialog;
use crate::webapp;

pub struct MainWindow {
    pub window: ApplicationWindow,
    list: ListBox,
    empty_stack: gtk::Stack,
    toast_overlay: ToastOverlay,
    apps: Rc<RefCell<Vec<DesktopEntry>>>,
}

impl MainWindow {
    pub fn new(app: &Application) -> Rc<Self> {
        let toast_overlay = ToastOverlay::new();

        let list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["boxed-list"])
            .build();
        list.set_can_focus(true);
        list.set_focusable(true);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .child(&list)
            .build();

        let empty = StatusPage::builder()
            .icon_name("web-browser-symbolic")
            .title("No web apps yet")
            .description(
                "Anchor turns any website into a desktop app with its own icon and browser profile.\n\nPress Ctrl+N to add one, or F1 for keyboard shortcuts.",
            )
            .build();

        let empty_stack = gtk::Stack::new();
        empty_stack.add_named(&empty, Some("empty"));
        empty_stack.add_named(&scrolled, Some("list"));

        let content = GtkBox::new(Orientation::Vertical, 0);
        content.append(&empty_stack);

        let clamp = libadwaita::Clamp::builder()
            .maximum_size(640)
            .child(&content)
            .build();

        let outer = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .vexpand(true)
            .build();
        outer.append(&clamp);

        toast_overlay.set_child(Some(&outer));

        let header = HeaderBar::new();
        let title = libadwaita::WindowTitle::new("Anchor", "Web apps for your desktop");
        header.set_title_widget(Some(&title));

        let add_btn = Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add Web App (Ctrl+N)")
            .css_classes(["suggested-action"])
            .build();
        header.pack_start(&add_btn);

        let shortcuts_btn = Button::builder()
            .icon_name("input-keyboard-symbolic")
            .tooltip_text("Keyboard shortcuts (F1)")
            .build();
        header.pack_end(&shortcuts_btn);

        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh list (Ctrl+R)")
            .build();
        header.pack_end(&refresh_btn);

        let toolbar = ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&toast_overlay));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Anchor")
            .default_width(640)
            .default_height(720)
            .content(&toolbar)
            .build();

        let this = Rc::new(Self {
            window,
            list,
            empty_stack,
            toast_overlay,
            apps: Rc::new(RefCell::new(Vec::new())),
        });

        {
            let this_c = Rc::clone(&this);
            add_btn.connect_clicked(move |_| this_c.open_create_dialog());
        }
        {
            let this_c = Rc::clone(&this);
            refresh_btn.connect_clicked(move |_| this_c.reload());
        }
        {
            let this_c = Rc::clone(&this);
            shortcuts_btn.connect_clicked(move |_| this_c.show_shortcuts());
        }

        // Activate row with double-click → launch
        {
            let this_c = Rc::clone(&this);
            this.list.connect_row_activated(move |_, _| {
                this_c.launch_selected();
            });
        }

        // List-local keys (must not be app-global — would break typing in dialogs)
        {
            let this_c = Rc::clone(&this);
            let keys = EventControllerKey::new();
            keys.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter {
                    this_c.launch_selected();
                    return glib::Propagation::Stop;
                }
                if keyval == gdk::Key::Delete || keyval == gdk::Key::KP_Delete {
                    this_c.delete_selected();
                    return glib::Propagation::Stop;
                }
                if keyval == gdk::Key::j || keyval == gdk::Key::J {
                    this_c.move_selection(1);
                    return glib::Propagation::Stop;
                }
                if keyval == gdk::Key::k || keyval == gdk::Key::K {
                    this_c.move_selection(-1);
                    return glib::Propagation::Stop;
                }
                // e without modifier when list focused → edit
                if keyval == gdk::Key::e || keyval == gdk::Key::E {
                    this_c.edit_selected();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            this.list.add_controller(keys);
        }

        this.install_actions(app);
        this.reload();
        this
    }

    fn install_actions(self: &Rc<Self>, app: &Application) {
        // win.* actions for keyboard accelerators (modifier-based so dialogs stay usable)
        let add = gio::SimpleAction::new("add", None);
        {
            let this = Rc::clone(self);
            add.connect_activate(move |_, _| this.open_create_dialog());
        }
        self.window.add_action(&add);

        let refresh = gio::SimpleAction::new("refresh", None);
        {
            let this = Rc::clone(self);
            refresh.connect_activate(move |_, _| this.reload());
        }
        self.window.add_action(&refresh);

        let launch = gio::SimpleAction::new("launch", None);
        {
            let this = Rc::clone(self);
            launch.connect_activate(move |_, _| this.launch_selected());
        }
        self.window.add_action(&launch);

        let edit = gio::SimpleAction::new("edit", None);
        {
            let this = Rc::clone(self);
            edit.connect_activate(move |_, _| this.edit_selected());
        }
        self.window.add_action(&edit);

        let delete = gio::SimpleAction::new("delete", None);
        {
            let this = Rc::clone(self);
            delete.connect_activate(move |_, _| this.delete_selected());
        }
        self.window.add_action(&delete);

        let shortcuts = gio::SimpleAction::new("shortcuts", None);
        {
            let this = Rc::clone(self);
            shortcuts.connect_activate(move |_, _| this.show_shortcuts());
        }
        self.window.add_action(&shortcuts);

        // Accelerators (Primary = Ctrl on Linux). Avoid bare Return/j/k globally.
        app.set_accels_for_action("win.add", &["<Primary>n"]);
        app.set_accels_for_action("win.refresh", &["<Primary>r", "F5"]);
        app.set_accels_for_action("win.edit", &["<Primary>e"]);
        app.set_accels_for_action("win.delete", &["<Primary>d", "<Primary>Delete"]);
        app.set_accels_for_action("win.shortcuts", &["F1", "<Primary>question"]);
        app.set_accels_for_action("win.launch", &["<Primary>Return"]);
    }

    pub fn present(&self) {
        self.window.present();
        let list = self.list.clone();
        glib::idle_add_local_once(move || {
            let _ = list.grab_focus();
        });
    }

    pub fn toast(&self, msg: &str) {
        self.toast_overlay.add_toast(Toast::new(msg));
    }

    pub fn reload(self: &Rc<Self>) {
        let selected_codename = self
            .selected_app()
            .map(|a| a.codename.clone());

        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        match webapp::list_webapps() {
            Ok(apps) => {
                let empty = apps.is_empty();
                *self.apps.borrow_mut() = apps.clone();
                if empty {
                    self.empty_stack.set_visible_child_name("empty");
                } else {
                    self.empty_stack.set_visible_child_name("list");
                    let mut restore_row: Option<ListBoxRow> = None;
                    for app in &apps {
                        let row = self.build_row(app);
                        if selected_codename.as_deref() == Some(app.codename.as_str()) {
                            restore_row = Some(row.clone());
                        }
                        self.list.append(&row);
                    }
                    if let Some(row) = restore_row {
                        self.list.select_row(Some(&row));
                    } else if let Some(first) = self.list.row_at_index(0) {
                        self.list.select_row(Some(&first));
                    }
                    let list = self.list.clone();
                    glib::idle_add_local_once(move || {
                        let _ = list.grab_focus();
                    });
                }
            }
            Err(e) => {
                self.empty_stack.set_visible_child_name("empty");
                self.toast(&format!("Failed to load apps: {e}"));
            }
        }
    }

    fn selected_index(&self) -> Option<i32> {
        self.list.selected_row().map(|r| r.index())
    }

    fn selected_app(&self) -> Option<DesktopEntry> {
        let idx = self.selected_index()? as usize;
        self.apps.borrow().get(idx).cloned()
    }

    fn move_selection(&self, delta: i32) {
        if self.apps.borrow().is_empty() {
            return;
        }
        self.empty_stack.set_visible_child_name("list");
        let n = self.apps.borrow().len() as i32;
        let cur = self.selected_index().unwrap_or(-1);
        let next = if cur < 0 {
            if delta > 0 {
                0
            } else {
                n - 1
            }
        } else {
            (cur + delta).rem_euclid(n)
        };
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    fn launch_selected(&self) {
        let Some(app) = self.selected_app() else {
            self.toast("Select a web app first");
            return;
        };
        self.launch_app(&app);
    }

    fn launch_app(&self, app: &DesktopEntry) {
        match gio::DesktopAppInfo::from_filename(&app.path) {
            Some(info) => {
                if let Err(e) = info.launch(&[], gio::AppLaunchContext::NONE) {
                    self.toast(&format!("Launch failed: {e}"));
                }
            }
            None => self.toast("Could not read desktop entry"),
        }
    }

    fn edit_selected(self: &Rc<Self>) {
        let Some(app) = self.selected_app() else {
            self.toast("Select a web app first");
            return;
        };
        self.open_edit_dialog(app);
    }

    fn delete_selected(self: &Rc<Self>) {
        let Some(app) = self.selected_app() else {
            self.toast("Select a web app first");
            return;
        };
        self.confirm_delete(app);
    }

    fn confirm_delete(self: &Rc<Self>, app_entry: DesktopEntry) {
        let delete_body = match app_entry.profile_mode {
            ProfileMode::Shared => {
                "This deletes the launcher, icon, and private copy of browser data used by this app. Your main browser profile is not modified."
            }
            ProfileMode::Isolated => {
                "This deletes the launcher, icon, and private browser profile. You will need to sign in again if you recreate it."
            }
        };
        let dialog = libadwaita::AlertDialog::builder()
            .heading(format!("Remove {}?", app_entry.name))
            .body(delete_body)
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Remove");
        dialog.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = Rc::clone(self);
        dialog.connect_response(None, move |_, response| {
            if response != "delete" {
                return;
            }
            match webapp::delete_webapp(&app_entry) {
                Ok(()) => {
                    this.toast(&format!("Removed {}", app_entry.name));
                    this.reload();
                }
                Err(e) => this.toast(&format!("Delete failed: {e}")),
            }
        });

        dialog.present(Some(self.window.upcast_ref::<gtk::Widget>()));
    }

    fn show_shortcuts(&self) {
        // Floating + explicit height so the dialog is not clipped to a short
        // parent window; the body scrolls when content is taller.
        let dialog = libadwaita::Dialog::builder()
            .title("Keyboard Shortcuts")
            .content_width(440)
            .content_height(520)
            .presentation_mode(libadwaita::DialogPresentationMode::Floating)
            .build();

        let page = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(18)
            .margin_end(18)
            .build();

        let general = PreferencesGroup::builder().title("General").build();
        for (title, accel) in [
            ("Add web app", "Ctrl+N"),
            ("Refresh list", "Ctrl+R  or  F5"),
            ("Keyboard shortcuts", "F1  or  Ctrl+?"),
            ("Close dialog", "Esc"),
        ] {
            general.add(&shortcut_row(title, accel));
        }

        let list = PreferencesGroup::builder()
            .title("Web app list")
            .description("Focus the list first (click a row or use ↑/↓).")
            .build();
        for (title, accel) in [
            ("Move selection down", "↓  or  J"),
            ("Move selection up", "↑  or  K"),
            ("Launch selected", "Enter"),
            ("Edit selected", "Ctrl+E"),
            ("Remove selected", "Delete  or  Ctrl+D"),
        ] {
            list.add(&shortcut_row(title, accel));
        }

        let form = PreferencesGroup::builder()
            .title("Add / Edit dialog")
            .build();
        for (title, accel) in [
            ("Next field", "Tab"),
            ("Previous field", "Shift+Tab"),
            ("Create / Save", "Enter (in Name or URL)"),
            ("Cancel", "Esc"),
        ] {
            form.add(&shortcut_row(title, accel));
        }

        page.append(&general);
        page.append(&list);
        page.append(&form);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .propagate_natural_width(true)
            .vexpand(true)
            .hexpand(true)
            .child(&page)
            .build();

        let close = Button::builder()
            .label("Close")
            .css_classes(["pill", "suggested-action"])
            .halign(Align::End)
            .build();
        {
            let dialog = dialog.clone();
            close.connect_clicked(move |_| {
                dialog.close();
            });
        }

        let footer = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .halign(Align::End)
            .margin_top(8)
            .margin_bottom(12)
            .margin_start(18)
            .margin_end(18)
            .build();
        footer.append(&close);

        let outer = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .build();
        outer.append(&scrolled);
        outer.append(&footer);

        dialog.set_child(Some(&outer));
        dialog.present(Some(self.window.upcast_ref::<gtk::Widget>()));
    }

    fn build_row(self: &Rc<Self>, app: &DesktopEntry) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.set_activatable(true);
        row.set_selectable(true);

        let icon_path = webapp::resolve_icon_file(&app.codename, &app.icon);
        let icon = if icon_path.is_file() {
            gtk::Image::from_file(&icon_path)
        } else if !app.icon.is_empty() && !app.icon.contains('/') {
            gtk::Image::from_icon_name(&app.icon)
        } else {
            gtk::Image::from_icon_name("web-browser-symbolic")
        };
        icon.set_pixel_size(40);

        let title = Label::builder()
            .label(&app.name)
            .halign(Align::Start)
            .hexpand(true)
            .css_classes(["title-3"])
            .build();
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let subtitle = Label::builder()
            .label(format!("{} · {}", app.browser, app.url))
            .halign(Align::Start)
            .css_classes(["dim-label", "caption"])
            .build();
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let text_col = GtkBox::new(Orientation::Vertical, 2);
        text_col.append(&title);
        text_col.append(&subtitle);
        text_col.set_hexpand(true);
        text_col.set_valign(Align::Center);

        let launch_btn = Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Launch (Enter)")
            .valign(Align::Center)
            .css_classes(["flat"])
            .build();

        let edit_btn = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit (Ctrl+E)")
            .valign(Align::Center)
            .css_classes(["flat"])
            .build();

        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove (Delete)")
            .valign(Align::Center)
            .css_classes(["flat"])
            .build();

        let hbox = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(8)
            .build();
        hbox.append(&icon);
        hbox.append(&text_col);
        hbox.append(&launch_btn);
        hbox.append(&edit_btn);
        hbox.append(&delete_btn);

        row.set_child(Some(&hbox));

        {
            let app = app.clone();
            let this = Rc::clone(self);
            launch_btn.connect_clicked(move |_| this.launch_app(&app));
        }
        {
            let app = app.clone();
            let this = Rc::clone(self);
            edit_btn.connect_clicked(move |_| this.open_edit_dialog(app.clone()));
        }
        {
            let app = app.clone();
            let this = Rc::clone(self);
            delete_btn.connect_clicked(move |_| this.confirm_delete(app.clone()));
        }

        row
    }

    fn open_create_dialog(self: &Rc<Self>) {
        let this = Rc::clone(self);
        CreateDialog::show(&self.window, move |result| match result {
            Ok(entry) => {
                this.toast(&format!("Installed {}", entry.name));
                this.reload();
            }
            Err(e) => this.toast(&format!("Could not create app: {e}")),
        });
    }

    fn open_edit_dialog(self: &Rc<Self>, app_entry: DesktopEntry) {
        let this = Rc::clone(self);
        CreateDialog::show_edit(&self.window, app_entry, move |result| match result {
            Ok(entry) => {
                this.toast(&format!("Updated {}", entry.name));
                this.reload();
            }
            Err(e) => this.toast(&format!("Could not update app: {e}")),
        });
    }
}

fn shortcut_row(title: &str, accelerator: &str) -> ActionRow {
    let row = ActionRow::builder().title(title).build();
    let label = Label::builder()
        .label(accelerator)
        .css_classes(["dim-label", "monospace"])
        .halign(Align::End)
        .build();
    row.add_suffix(&label);
    row.set_activatable(false);
    row
}
