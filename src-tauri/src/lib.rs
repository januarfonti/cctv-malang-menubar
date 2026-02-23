mod proxy;

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    window::{Effect, EffectsBuilder},
    Manager,
};
use tauri_plugin_positioner::{Position, WindowExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            tauri::async_runtime::spawn(async {
                proxy::start_proxy_server().await;
            });

            let _popup = WebviewWindowBuilder::new(
                app,
                "popup",
                tauri::WebviewUrl::App("/index.html".into()),
            )
            .title("CCTV Menubar")
            .inner_size(400.0, 380.0)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .focused(false)
            .transparent(true)
            .effects(
                EffectsBuilder::new()
                    .effect(Effect::Popover)
                    .radius(12.0)
                    .build(),
            )
            .build()?;

            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app).item(&quit).build()?;

            let tray_icon = TrayIconBuilder::new()
                .icon(Image::from_bytes(include_bytes!("../icons/tray-icon@2x.png"))?)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("CCTV Kota Malang")
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(popup) = app.get_webview_window("popup") {
                            if popup.is_visible().unwrap_or(false) {
                                let _ = popup.hide();
                            } else {
                                let _ = popup.move_window(Position::TrayBottomCenter);
                                let _ = popup.show();
                                let _ = popup.set_focus();
                            }
                        }
                    }
                })
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            app.manage(tray_icon);

            let app_handle = app.handle().clone();
            if let Some(popup) = app_handle.get_webview_window("popup") {
                popup.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        if let Some(w) = app_handle.get_webview_window("popup") {
                            let _ = w.hide();
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running menubar application");
}
