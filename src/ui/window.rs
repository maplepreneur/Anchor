//! Main application window: list web apps, add, delete.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib, Align, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow};
use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, HeaderBar, StatusPage, Toast, ToastOverlay, ToolbarView};

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
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .child(&list)
            .build();

        let empty = StatusPage::builder()
            .icon_name("web-browser-symbolic")
            .title("No web apps yet")
            .description("Anchor turns any website into a desktop app with its own icon and browser profile.")
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

        // Outer margin box
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
            .tooltip_text("Add Web App")
            .css_classes(["suggested-action"])
            .build();
        header.pack_start(&add_btn);

        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh list")
            .build();
        header.pack_end(&refresh_btn);

        let toolbar = ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&toast_overlay));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Anchor")
            .default_width(520)
            .default_height(560)
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
            add_btn.connect_clicked(move |_| {
                this_c.open_create_dialog();
            });
        }
        {
            let this_c = Rc::clone(&this);
            refresh_btn.connect_clicked(move |_| {
                this_c.reload();
            });
        }

        this.reload();
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn toast(&self, msg: &str) {
        self.toast_overlay.add_toast(Toast::new(msg));
    }

    pub fn reload(&self) {
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
                    for app in apps {
                        let row = self.build_row(&app);
                        self.list.append(&row);
                    }
                }
            }
            Err(e) => {
                self.empty_stack.set_visible_child_name("empty");
                self.toast(&format!("Failed to load apps: {e}"));
            }
        }
    }

    fn build_row(&self, app: &DesktopEntry) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.set_activatable(false);

        let icon = if !app.icon.is_empty() && std::path::Path::new(&app.icon).exists() {
            gtk::Image::from_file(&app.icon)
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
            .tooltip_text("Launch")
            .valign(Align::Center)
            .css_classes(["flat"])
            .build();

        let edit_btn = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit web app")
            .valign(Align::Center)
            .css_classes(["flat"])
            .build();

        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove web app")
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

        // Launch via gio::AppInfo from desktop file
        {
            let path = app.path.clone();
            let toast = self.toast_overlay.clone();
            launch_btn.connect_clicked(move |_| {
                match gio::DesktopAppInfo::from_filename(&path) {
                    Some(info) => {
                        if let Err(e) = info.launch(&[], gio::AppLaunchContext::NONE) {
                            toast.add_toast(Toast::new(&format!("Launch failed: {e}")));
                        }
                    }
                    None => {
                        toast.add_toast(Toast::new("Could not read desktop entry"));
                    }
                }
            });
        }

        {
            let app_entry = app.clone();
            let parent_window = self.window.clone();
            let toast_overlay = self.toast_overlay.clone();
            edit_btn.connect_clicked(move |_| {
                let toast_overlay = toast_overlay.clone();
                CreateDialog::show_edit(&parent_window, app_entry.clone(), move |result| {
                    match result {
                        Ok(entry) => {
                            toast_overlay
                                .add_toast(Toast::new(&format!("Updated {}", entry.name)));
                            if let Some(app) = gio::Application::default() {
                                app.activate_action("refresh", None);
                            }
                        }
                        Err(e) => {
                            toast_overlay
                                .add_toast(Toast::new(&format!("Could not update app: {e}")));
                        }
                    }
                });
            });
        }

        {
            let app_entry = app.clone();
            // We need a weak-ish way to call reload: store window handle via clone of list parent
            // Use glib::clone with a clone of the MainWindow pieces
            let list = self.list.clone();
            let empty_stack = self.empty_stack.clone();
            let apps_store = Rc::clone(&self.apps);
            let toast_overlay = self.toast_overlay.clone();
            let parent_window = self.window.clone();

            delete_btn.connect_clicked(move |btn| {
                let app_entry = app_entry.clone();
                let list = list.clone();
                let empty_stack = empty_stack.clone();
                let apps_store = Rc::clone(&apps_store);
                let toast_overlay = toast_overlay.clone();

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

                let btn_window = btn.root().and_downcast::<gtk::Window>();
                let parent = btn_window.as_ref().map(|w| w.upcast_ref::<gtk::Widget>());

                dialog.connect_response(None, move |_, response| {
                    if response != "delete" {
                        return;
                    }
                    match webapp::delete_webapp(&app_entry) {
                        Ok(()) => {
                            toast_overlay.add_toast(Toast::new(&format!("Removed {}", app_entry.name)));
                            // Reload list
                            while let Some(child) = list.first_child() {
                                list.remove(&child);
                            }
                            match webapp::list_webapps() {
                                Ok(apps) => {
                                    let empty = apps.is_empty();
                                    *apps_store.borrow_mut() = apps.clone();
                                    if empty {
                                        empty_stack.set_visible_child_name("empty");
                                    } else {
                                        empty_stack.set_visible_child_name("list");
                                        // Rows will be incomplete if we only clear — parent must rebuild.
                                        // Emit a simple full rebuild by re-calling via timeout on main context
                                        // using a custom signal is overkill; rebuild rows here is hard without MainWindow.
                                        // Schedule app-level refresh:
                                        glib::idle_add_local_once(|| {
                                            // no-op placeholder; full rebuild happens below
                                        });
                                    }
                                    // Rebuild rows from apps_store by synthesizing via list_webapps again in window
                                    // We'll use Application action instead.
                                    if let Some(app) = gio::Application::default() {
                                        app.activate_action("refresh", None);
                                    }
                                    let _ = empty; // silence if unused path
                                }
                                Err(e) => {
                                    toast_overlay.add_toast(Toast::new(&format!("Error: {e}")));
                                }
                            }
                        }
                        Err(e) => {
                            toast_overlay.add_toast(Toast::new(&format!("Delete failed: {e}")));
                        }
                    }
                });

                if let Some(w) = parent {
                    dialog.present(Some(w));
                } else {
                    dialog.present(Some(parent_window.upcast_ref::<gtk::Widget>()));
                }
            });
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
            Err(e) => {
                this.toast(&format!("Could not create app: {e}"));
            }
        });
    }
}
