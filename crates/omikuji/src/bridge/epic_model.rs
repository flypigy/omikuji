#![allow(clippy::too_many_arguments)]

use super::store_model;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};
use omikuji_core::components;
use omikuji_core::library::{Library, SourceKind};
use omikuji_core::store::StoreGame;
use omikuji_core::store::epic::{self, EpicStore};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

static EPIC_STORE: LazyLock<Mutex<EpicStore>> = LazyLock::new(|| Mutex::new(EpicStore::new()));

// a full `legendary list` costs tens of seconds on large libraries, so repeat
// refreshes inside this window reuse the in-memory model instead
const REFRESH_TTL: Duration = Duration::from_secs(5 * 60);

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(bool, is_logged_in, cxx_name = "isLoggedIn")]
        #[qproperty(bool, is_refreshing, cxx_name = "isRefreshing")]
        #[qproperty(QString, display_name, cxx_name = "displayName")]
        #[qproperty(bool, tool_ready, cxx_name = "toolReady")]
        #[qproperty(bool, tool_installing, cxx_name = "toolInstalling")]
        type EpicModel = super::EpicModelRust;
    }

    unsafe extern "RustQt" {
        #[cxx_name = "rowCount"]
        #[cxx_override]
        fn row_count(self: &EpicModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(self: &EpicModel, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_name = "roleNames"]
        #[cxx_override]
        fn role_names(self: &EpicModel) -> QHash_i32_QByteArray;

        #[qinvokable]
        fn get_login_url(self: &EpicModel) -> QString;

        #[qinvokable]
        fn login(self: Pin<&mut EpicModel>, code: &QString);

        #[qinvokable]
        fn logout(self: Pin<&mut EpicModel>);

        #[qinvokable]
        fn refresh(self: Pin<&mut EpicModel>);

        #[qinvokable]
        fn force_refresh(self: Pin<&mut EpicModel>);

        #[qinvokable]
        fn set_search(self: Pin<&mut EpicModel>, text: &QString);

        #[qinvokable]
        fn enqueue_install(
            self: Pin<&mut EpicModel>,
            index: i32,
            install_path: &QString,
            prefix_path: &QString,
            runner_version: &QString,
            is_import: bool,
            import_existing: bool,
            dlcs: &QString,
        ) -> QString;

        #[qinvokable]
        fn get_game_at(self: &EpicModel, index: i32) -> QMap_QString_QVariant;

        #[qinvokable]
        fn install_tools(self: Pin<&mut EpicModel>);

        #[qinvokable]
        fn refresh_tools(self: Pin<&mut EpicModel>);
    }

    unsafe extern "RustQt" {
        #[cxx_name = "beginResetModel"]
        #[inherit]
        fn begin_reset_model(self: Pin<&mut EpicModel>);

        #[cxx_name = "endResetModel"]
        #[inherit]
        fn end_reset_model(self: Pin<&mut EpicModel>);
    }

    impl cxx_qt::Threading for EpicModel {}
}

pub struct EpicModelRust {
    // `games` is the search-filtered view the ui reads; `all_games` keeps the full
    // library so filtering never needs another legendary run
    pub games: Vec<StoreGame>,
    pub all_games: Vec<StoreGame>,
    pub search: String,
    // app_name -> library game id, drives the three-state card ui and attributes downloads
    pub library_ids: HashMap<String, String>,
    pub is_logged_in: bool,
    pub is_refreshing: bool,
    pub display_name: QString,
    pub tool_ready: bool,
    pub tool_installing: bool,
    pub last_refresh: Option<Instant>,
}

impl Default for EpicModelRust {
    fn default() -> Self {
        // blocking_lock would panic inside the tokio runtime; try_lock is safe at startup
        let (is_logged_in, display_name) = match EPIC_STORE.try_lock() {
            Ok(store) => (store.is_logged_in(), QString::from(&store.display_name)),
            Err(_) => (false, QString::default()),
        };

        Self {
            games: Vec::new(),
            all_games: Vec::new(),
            search: String::new(),
            library_ids: HashMap::new(),
            is_logged_in,
            is_refreshing: false,
            display_name,
            tool_ready: components::ready(&components::epic_tools()),
            tool_installing: false,
            last_refresh: None,
        }
    }
}

impl qobject::EpicModel {
    pub fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().games.len() as i32
    }

    pub fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        store_model::role_names()
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        store_model::role_data(
            &self.rust().games,
            &self.rust().library_ids,
            index.row(),
            role,
        )
    }

    pub fn get_login_url(&self) -> QString {
        QString::from(&EpicStore::get_login_url())
    }

    pub fn install_tools(mut self: Pin<&mut Self>) {
        if self.rust().tool_installing {
            return;
        }
        self.as_mut().set_tool_installing(true);
        let qt_thread = self.as_mut().qt_thread();
        tokio::spawn(async move {
            let ok = components::ensure(&components::epic_tools())
                .await
                .map_err(|e| tracing::error!("epic tools install failed: {}", e))
                .is_ok();
            let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                obj.as_mut().set_tool_installing(false);
                obj.as_mut().set_tool_ready(ok);
            });
        });
    }

    pub fn refresh_tools(mut self: Pin<&mut Self>) {
        let ready = components::ready(&components::epic_tools());
        self.as_mut().set_tool_ready(ready);
    }

    pub fn login(mut self: Pin<&mut Self>, code: &QString) {
        let code_str = code.to_string();
        let qt_thread = self.as_mut().qt_thread();

        tokio::spawn(async move {
            let result = {
                let mut store = EPIC_STORE.lock().await;
                store.login(&code_str).await
            };

            match result {
                Ok(name) => {
                    let display_name = QString::from(&name);
                    let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                        obj.as_mut().set_is_logged_in(true);
                        obj.as_mut().set_display_name(display_name);
                        obj.as_mut().force_refresh();
                    });
                }
                Err(e) => {
                    tracing::error!("login failed: {}", e);
                }
            }
        });
    }

    pub fn logout(mut self: Pin<&mut Self>) {
        let qt_thread = self.as_mut().qt_thread();
        tokio::spawn(async move {
            {
                let mut store = EPIC_STORE.lock().await;
                if let Err(e) = store.logout().await {
                    tracing::error!("logout failed: {}", e);
                }
            }
            let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                obj.as_mut().set_is_logged_in(false);
                obj.as_mut().set_display_name(QString::default());
                obj.as_mut().begin_reset_model();
                let rust = obj.as_mut().rust_mut().get_mut();
                rust.games.clear();
                rust.all_games.clear();
                rust.search.clear();
                rust.library_ids.clear();
                rust.last_refresh = None;
                obj.as_mut().end_reset_model();
            });
        });
    }

    pub fn refresh(self: Pin<&mut Self>) {
        self.internal_refresh(false);
    }

    pub fn force_refresh(self: Pin<&mut Self>) {
        self.internal_refresh(true);
    }

    fn internal_refresh(mut self: Pin<&mut Self>, force: bool) {
        let skip = {
            let rust = self.rust();
            rust.is_refreshing
                || (!force
                    && rust.last_refresh.is_some_and(|t| t.elapsed() < REFRESH_TTL)
                    && !rust.all_games.is_empty())
        };
        if skip {
            return;
        }
        self.as_mut().set_is_refreshing(true);
        let qt_thread = self.as_mut().qt_thread();

        tokio::spawn(async move {
            let (cached, ids_pre) = tokio::task::spawn_blocking(|| {
                let games = epic::load_cached_library();
                let ids = Library::game_ids_by_app_id(SourceKind::Epic);
                (games, ids)
            })
            .await
            .unwrap_or_default();

            if !cached.is_empty() {
                let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                    if !obj.as_ref().all_games.is_empty() {
                        return;
                    }
                    obj.as_mut().begin_reset_model();
                    let rust = obj.as_mut().rust_mut().get_mut();
                    rust.all_games = cached;
                    rust.games = store_model::filtered_games(&rust.all_games, &rust.search);
                    rust.library_ids = ids_pre;
                    obj.as_mut().end_reset_model();
                });
            }

            let result = {
                let mut store = EPIC_STORE.lock().await;
                store.list_games().await
            };

            match result {
                Ok(games) => {
                    let ids = tokio::task::spawn_blocking(|| {
                        Library::game_ids_by_app_id(SourceKind::Epic)
                    })
                    .await
                    .unwrap_or_default();

                    let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                        let search = obj.as_ref().search.clone();
                        let filtered = store_model::filtered_games(&games, &search);
                        let unchanged = obj.as_ref().all_games == games
                            && obj.as_ref().games == filtered
                            && obj.as_ref().library_ids == ids;
                        if unchanged {
                            let rust = obj.as_mut().rust_mut().get_mut();
                            rust.last_refresh = Some(Instant::now());
                            obj.as_mut().set_is_refreshing(false);
                            return;
                        }
                        obj.as_mut().begin_reset_model();
                        let rust = obj.as_mut().rust_mut().get_mut();
                        rust.all_games = games;
                        rust.games = filtered;
                        rust.library_ids = ids;
                        rust.last_refresh = Some(Instant::now());
                        obj.as_mut().end_reset_model();
                        obj.as_mut().set_is_refreshing(false);
                    });
                }
                Err(e) => {
                    tracing::error!("refresh failed: {}", e);
                    let _ = qt_thread.queue(move |mut obj: Pin<&mut qobject::EpicModel>| {
                        obj.as_mut().set_is_refreshing(false);
                    });
                }
            }
        });
    }

    pub fn set_search(mut self: Pin<&mut Self>, text: &QString) {
        let query = text.to_string().trim().to_lowercase();
        let filtered = {
            let rust = self.as_mut().rust_mut().get_mut();
            if rust.search == query {
                return;
            }
            rust.search = query;
            store_model::filtered_games(&rust.all_games, &rust.search)
        };
        let unchanged = { self.rust().games == filtered };
        if unchanged {
            return;
        }
        self.as_mut().begin_reset_model();
        let rust = self.as_mut().rust_mut().get_mut();
        rust.games = filtered;
        self.as_mut().end_reset_model();
    }

    pub fn enqueue_install(
        self: Pin<&mut Self>,
        index: i32,
        install_path: &QString,
        prefix_path: &QString,
        runner_version: &QString,
        is_import: bool,
        import_existing: bool,
        dlcs: &QString,
    ) -> QString {
        let Some(game) = usize::try_from(index)
            .ok()
            .and_then(|i| self.rust().games.get(i))
        else {
            tracing::error!("enqueue_install: bad index {}", index);
            return QString::default();
        };

        let game_id = self
            .rust()
            .library_ids
            .get(&game.app_name)
            .cloned()
            .unwrap_or_default();

        store_model::enqueue_install(
            "epic",
            game,
            &store_model::InstallOptions {
                install_path,
                prefix_path,
                runner_version,
                is_import,
                import_existing,
                dlcs,
                game_id: &game_id,
            },
        )
    }

    pub fn get_game_at(
        &self,
        index: i32,
    ) -> cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant> {
        store_model::game_map(&self.rust().games, &self.rust().library_ids, index)
    }
}
