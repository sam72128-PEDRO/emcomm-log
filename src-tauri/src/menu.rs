use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Wry};

/// Native menus. Item ids are forwarded to the UI as `menu` events; the UI owns the actions so
/// menu clicks and keyboard shortcuts run the same code path.
pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, label: &str, accel: Option<&str>| {
        let mut b = MenuItemBuilder::with_id(id, label);
        if let Some(a) = accel {
            b = b.accelerator(a);
        }
        b.build(app)
    };
    let settings = item("settings", "Settings…", Some("CmdOrCtrl+,"))?;
    let quit = item("quit", if cfg!(target_os = "macos") { "Quit EMCOMM LOG" } else { "Exit" }, Some("CmdOrCtrl+Q"))?;

    let mut file = SubmenuBuilder::new(app, "File")
        .item(&item("new_entry", "New Entry", Some("CmdOrCtrl+N"))?)
        .item(&item("save_entry", "Save Entry / Save Changes", Some("CmdOrCtrl+Enter"))?)
        .separator()
        .item(&item("logbooks", "Logbooks…", Some("CmdOrCtrl+Shift+O"))?)
        .item(&item("close_logbook", "Close Logbook", None)?)
        .separator()
        .item(&item("export", "Export…", Some("CmdOrCtrl+E"))?)
        .item(&item("backup_now", "Back Up Now", Some("CmdOrCtrl+Shift+B"))?)
        .item(&item("import_package", "Import Package…", None)?)
        .item(&item("restore_backup", "Restore Backup…", None)?);
    if !cfg!(target_os = "macos") {
        file = file.separator().item(&settings).separator().item(&quit);
    }
    let file = file.build()?;

    let edit = SubmenuBuilder::new(app, "Edit").undo().redo().separator().cut().copy().paste().select_all().build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("nav_dashboard", "Dashboard", Some("CmdOrCtrl+1"))?)
        .item(&item("nav_quick_entry", "Quick Entry", Some("CmdOrCtrl+2"))?)
        .item(&item("nav_traffic_log", "Traffic Log", Some("CmdOrCtrl+3"))?)
        .item(&item("nav_messages", "Messages", Some("CmdOrCtrl+4"))?)
        .item(&item("nav_reports", "Reports & Export", Some("CmdOrCtrl+5"))?)
        .separator()
        .item(&item("find", "Find in Traffic Log", Some("CmdOrCtrl+F"))?)
        .item(&item("toggle_theme", "Toggle Light/Dark Theme", None)?)
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&item("help_shortcuts", "Keyboard Shortcuts", Some("CmdOrCtrl+/"))?)
        .item(&item("help_data", "Where Is My Data?", None)?)
        .item(&item("help_about", "About EMCOMM LOG", None)?)
        .build()?;

    let mut menu = MenuBuilder::new(app);
    if cfg!(target_os = "macos") {
        let app_menu = SubmenuBuilder::new(app, "EMCOMM LOG")
            .item(&item("help_about", "About EMCOMM LOG", None)?)
            .separator()
            .item(&settings)
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .item(&quit)
            .build()?;
        menu = menu.item(&app_menu);
    }
    menu.item(&file).item(&edit).item(&view).item(&help).build()
}
