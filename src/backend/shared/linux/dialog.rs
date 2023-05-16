//! This module contains functions for opening file dialogs using DBus.

use ashpd::desktop::file_chooser;
use ashpd::WindowIdentifier;
use futures::executor::block_on;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use tracing::warn;

use crate::{FileDialogOptions, FileDialogToken, FileInfo};

use crate::window::IdleHandle;

pub(crate) fn open_file(
    window: RawWindowHandle,
    display: RawDisplayHandle,
    idle: IdleHandle,
    options: FileDialogOptions,
) -> FileDialogToken {
    dialog(window, display, idle, options, true)
}

pub(crate) fn save_file(
    window: RawWindowHandle,
    display: RawDisplayHandle,
    idle: IdleHandle,
    options: FileDialogOptions,
) -> FileDialogToken {
    dialog(window, display, idle, options, false)
}

fn dialog(
    window: RawWindowHandle,
    display: RawDisplayHandle,
    idle: IdleHandle,
    mut options: FileDialogOptions,
    open: bool,
) -> FileDialogToken {
    let tok = FileDialogToken::next();

    let id = block_on(async { WindowIdentifier::from_raw_handle(&window, Some(&display)).await });
    std::thread::spawn(move || {
        if let Err(e) = block_on(async {
            let multi = options.multi_selection;

            let title_owned = options.title.take();
            let title = match (open, options.select_directories) {
                (true, true) => "Open Folder",
                (true, false) => "Open File",
                (false, _) => "Save File",
            };
            let title = title_owned.as_deref().unwrap_or(title);
            let open_result: file_chooser::SelectedFiles;
            let save_result;
            let uris = if open {
                let mut request = file_chooser::SelectedFiles::open_file()
                    .title(title)
                    .identifier(Some(id))
                    .modal(Some(true));
                if let Some(label) = &options.button_text {
                    request = request.accept_label(Some(label.as_str()));
                }

                if let Some(filters) = options.allowed_types {
                    for f in filters {
                        request = request.filter(f.into());
                    }
                }

                if let Some(filter) = options.default_type {
                    request = request.current_filter(Some(filter.into()));
                }
                open_result = request.send().await?.response()?;
                open_result.uris()
            } else {
                let mut request = file_chooser::SelectedFiles::save_file()
                    .identifier(Some(id))
                    .title(title)
                    .modal(true);

                if let Some(name) = &options.default_name {
                    request = request.current_name(Some(name.as_str()));
                }

                if let Some(label) = &options.button_text {
                    request = request.accept_label(Some(label.as_str()));
                }

                if let Some(filters) = options.allowed_types {
                    for f in filters {
                        request = request.filter(f.into());
                    }
                }

                if let Some(filter) = options.default_type {
                    request = request.current_filter(Some(filter.into()));
                }

                if let Some(dir) = &options.starting_directory {
                    request = request.current_folder(dir)?;
                }

                save_result = request.send().await?.response()?;
                save_result.uris()
            };

            let mut paths = uris.iter().filter_map(|s| s.to_file_path().ok());
            if multi && open {
                let infos = paths
                    .map(|p| FileInfo {
                        path: p,
                        format: None,
                    })
                    .collect();
                idle.add_idle(move |handler| handler.open_files(tok, infos));
            } else if !multi {
                if uris.len() > 2 {
                    warn!(
                        "expected one path (got {}), returning only the first",
                        uris.len()
                    );
                }
                let info = paths.next().map(|p| FileInfo {
                    path: p,
                    format: None,
                });
                if open {
                    idle.add_idle(move |handler| handler.open_file(tok, info));
                } else {
                    idle.add_idle(move |handler| handler.save_as(tok, info));
                }
            } else {
                warn!("cannot save multiple paths");
            }

            Ok(()) as ashpd::Result<()>
        }) {
            warn!("error while opening file dialog: {}", e);
        }
    });

    tok
}

impl From<crate::FileSpec> for file_chooser::FileFilter {
    fn from(spec: crate::FileSpec) -> file_chooser::FileFilter {
        let mut filter = file_chooser::FileFilter::new(spec.name);
        for ext in spec.extensions {
            filter = filter.glob(&format!("*.{ext}"));
        }
        filter
    }
}
