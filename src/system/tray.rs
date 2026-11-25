use tray_icon::{
    menu::{Menu, MenuId, MenuItem},
    TrayIconBuilder,
};

#[derive(Clone)]
pub struct TrayMenuIds {
    pub open: MenuId,
    pub toggle_hotkeys: MenuId,
    pub stop_all: MenuId,
    pub quit: MenuId,
}

impl TrayMenuIds {
    pub fn new() -> Self {
        Self {
            open: MenuId::new("soundboard-open"),
            toggle_hotkeys: MenuId::new("soundboard-toggle-hotkeys"),
            stop_all: MenuId::new("soundboard-stop-all"),
            quit: MenuId::new("soundboard-quit"),
        }
    }
}

pub struct SystemTray {
    _tray_icon: tray_icon::TrayIcon,
}

impl SystemTray {
    pub fn new(ids: &TrayMenuIds) -> Result<Self, String> {
        let open = MenuItem::with_id(ids.open.clone(), "Abrir / Mostrar ventana", true, None);
        let toggle = MenuItem::with_id(
            ids.toggle_hotkeys.clone(),
            "Pausar / Reanudar atajos",
            true,
            None,
        );
        let stop = MenuItem::with_id(
            ids.stop_all.clone(),
            "Detener todos los sonidos",
            true,
            None,
        );
        let quit = MenuItem::with_id(ids.quit.clone(), "Salir", true, None);

        let menu = Menu::new();
        menu.append(&open).map_err(|error| format!("{error:?}"))?;
        menu.append(&toggle).map_err(|error| format!("{error:?}"))?;
        menu.append(&stop).map_err(|error| format!("{error:?}"))?;
        menu.append(&quit).map_err(|error| format!("{error:?}"))?;

        let (rgba, width, height) = crate::app_icon::rgba_icon(64)?;
        let icon = tray_icon::Icon::from_rgba(rgba, width, height)
            .map_err(|error| format!("{error:?}"))?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()
            .map_err(|error| format!("{error:?}"))?;

        Ok(Self {
            _tray_icon: tray_icon,
        })
    }
}
