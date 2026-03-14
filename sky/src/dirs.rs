use std::{ops::Deref, path::PathBuf, sync::LazyLock};

use etcetera::{choose_app_strategy, AppStrategy, AppStrategyArgs};

macro_rules! etc {
    () => {
        choose_app_strategy(AppStrategyArgs {
            top_level_domain: "dev".to_string(),
            author: "zephiris".to_string(),
            app_name: "sky".to_string(),
        })
        .unwrap()
    };
}

pub struct Dir {}

impl Dir {
    pub fn data() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().data_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
    pub fn config() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().config_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
    pub fn cache() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().cache_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
}
