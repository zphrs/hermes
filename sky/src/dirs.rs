use std::{ops::Deref, path::PathBuf, sync::LazyLock};

use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};

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
#[allow(dead_code)]
pub struct Dir;

impl Dir {
    #[allow(dead_code)]
    pub fn data() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().data_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
    #[allow(dead_code)]
    pub fn config() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().config_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
    #[allow(dead_code)]
    pub fn cache() -> &'static impl Deref<Target = PathBuf> {
        static ONCE_LOCK: LazyLock<PathBuf> = LazyLock::new(|| {
            let path = etc!().cache_dir();
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        &ONCE_LOCK
    }
}
