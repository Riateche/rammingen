#![allow(clippy::missing_safety_doc)]

use {
    anyhow::{Context as _, bail, format_err},
    byte_unit::Byte,
    cadd::prelude::IntoType as _,
    clap::Parser as _,
    jni::{
        JNIEnv,
        objects::{GlobalRef, JObject, JString, JValue},
        sys::{self, jboolean},
    },
    rammingen::{
        Secrets,
        cli::Cli,
        config::{
            Config, MountPoint, default_desktop_notification_interval, default_log_filter,
            default_warn_about_files_larger_than,
        },
        path::SanitizedLocalPath,
        rules::Rule,
        setup_logger,
        term::{Term, set_term, term},
    },
    rammingen_protocol::{AccessToken, ArchivePath, EncryptionKey, serde_path_with_prefix},
    scopeguard::defer,
    serde::{Deserialize, Serialize},
    std::{
        any::Any,
        cell::Cell,
        fmt::Display,
        panic,
        path::{Path, PathBuf},
        ptr,
        sync::Once,
        time::Duration,
    },
    tracing::{Level, error},
    url::Url,
};

thread_local!(static JNI_ENV: Cell<*mut sys::JNIEnv> = const { Cell::new(ptr::null_mut()) });

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_me_darkecho_rammingen_NativeBridge_run(
    mut env: JNIEnv,
    _class: JObject,
    app_dir: JString,
    storage_root: JString,
    config: JString,
    access_token: JString,
    encryption_key: JString,
    args: JString,
    log_receiver: JObject,
) -> jboolean {
    let log_receiver = env
        .new_global_ref(log_receiver)
        .unwrap_or_else(|e| env.fatal_error(format!("new_global_ref failed: {e:?}")));

    JNI_ENV.with(|v| v.set(env.get_raw()));
    defer!(JNI_ENV.with(|v| v.set(ptr::null_mut())));

    let native_term = NativeBridgeTerm { log_receiver };
    set_term(Some(Box::new(native_term)));

    let r = run(
        &mut env,
        app_dir,
        storage_root,
        config,
        access_token,
        encryption_key,
        args,
    );
    match r {
        Ok(()) => 1,
        Err(err) => {
            term().write(Level::ERROR, &format!("{err:?}"));
            0
        }
    }
}

fn setup_logger_and_panic_hook(
    log_file: Option<&Path>,
    log_filter: Option<&str>,
) -> anyhow::Result<()> {
    static ONCE: Once = Once::new();
    let mut setup_logger_result = Ok(());
    ONCE.call_once(|| {
        setup_logger_result = setup_logger(
            log_file.map(|p| p.to_owned()),
            log_filter
                .map(|p| p.to_owned())
                .unwrap_or_else(default_log_filter),
        );

        panic::set_hook(Box::new(|info| {
            error!(%info, "panic");
        }));
    });
    setup_logger_result
}

fn log_to_android(env: &mut JNIEnv<'_>, text: impl Display) {
    let log_class = env
        .find_class("android/util/Log")
        .unwrap_or_else(|e| env.fatal_error(format!("find_class(android/util/Log) failed: {e:?}")));
    let tag = env
        .new_string("rammingen_native")
        .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));
    let text = env
        .new_string(text.to_string())
        .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));

    env.call_static_method(
        &log_class,
        "d",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        &[JValue::Object(tag.as_ref()), JValue::Object(&text.into())],
    )
    .unwrap_or_else(|e| env.fatal_error(format!("Log.d failed: {e:?}")));
}

fn run(
    env: &mut JNIEnv<'_>,
    app_dir: JString,
    storage_root: JString,
    config: JString,
    access_token: JString,
    encryption_key: JString,
    args: JString,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let app_dir = env
        .get_string(&app_dir)
        .context("failed to get java string")?
        .into_type::<String>()
        .into_type::<PathBuf>();
    let storage_root = env
        .get_string(&storage_root)
        .context("failed to get java string")?
        .into_type::<String>()
        .into_type::<PathBuf>();
    let config = env
        .get_string(&config)
        .context("failed to get java string")?
        .into_type::<String>();
    let access_token = env
        .get_string(&access_token)
        .context("failed to get java string")?
        .into_type::<String>();
    let encryption_key = env
        .get_string(&encryption_key)
        .context("failed to get java string")?
        .into_type::<String>();
    let args = env
        .get_string(&args)
        .context("failed to get java string")?
        .into_type::<String>();

    let mut args = shell_words::split(&args).context("failed to parse args into words")?;
    args.insert(0, "rammingen".to_owned());
    let cli = Cli::try_parse_from(&args).context("failed to parse args")?;
    let config_content = if let Some(config_path) = cli.config {
        log_to_android(env, format!("using config at {:?}", config_path));
        fs_err::read_to_string(config_path)?
    } else {
        log_to_android(env, "using config content from argument");
        config
    };
    let config = prepare_config(&app_dir, &storage_root, &config_content)?;
    log_to_android(env, format!("config: {config:?}"));

    setup_logger_and_panic_hook(config.log_file.as_deref(), Some(&config.log_filter))?;

    panic::catch_unwind(|| {
        runtime.block_on(rammingen::run(
            cli.command,
            config,
            Some(Secrets {
                access_token: access_token.parse()?,
                encryption_key: encryption_key.parse()?,
            }),
        ))?;
        anyhow::Ok(())
    })
    .map_err(|err| format_err!("panic: {}", format_panic_message(err)))?
}

fn format_panic_message(err: Box<dyn Any + Send + 'static>) -> String {
    err.downcast_ref::<&'static str>()
        .map(|s| s.to_string())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| format!("{err:?}"))
}

fn level_to_i32(level: Level) -> i32 {
    if level == Level::TRACE {
        0
    } else if level == Level::DEBUG {
        1
    } else if level == Level::INFO {
        2
    } else if level == Level::WARN {
        3
    } else {
        4
    }
}

struct NativeBridgeTerm {
    log_receiver: GlobalRef,
}

fn with_jni_env(f: impl FnOnce(JNIEnv<'_>)) {
    JNI_ENV.with(|jni_env| {
        // Safety: JNI_ENV is cleared at the end of the native bridge call,
        // so it always contains either a valid pointer or a null pointer.
        let jni_env = unsafe { JNIEnv::from_raw(jni_env.get()) };
        if let Ok(jni_env) = jni_env {
            f(jni_env);
        }
    })
}

impl Term for NativeBridgeTerm {
    fn set_status(&mut self, status: &str) {
        with_jni_env(|mut env| {
            let status = env
                .new_string(status.to_string())
                .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));
            env.call_method(
                &self.log_receiver,
                "onNativeBridgeStatus",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&status)],
            )
            .unwrap_or_else(|e| env.fatal_error(format!("onNativeBridgeStatus failed: {e:?}")));
        });
    }

    fn clear_status(&mut self) {
        self.set_status("");
    }

    fn write(&mut self, level: Level, text: &str) {
        with_jni_env(|mut env| {
            log_to_android(&mut env, "ok before term write");
            let text = env
                .new_string(text.to_string())
                .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));
            env.call_method(
                &self.log_receiver,
                "onNativeBridgeLog",
                "(ILjava/lang/String;)V",
                &[JValue::Int(level_to_i32(level)), JValue::Object(&text)],
            )
            .unwrap_or_else(|e| env.fatal_error(format!("onNativeBridgeLog failed: {e:?}")));
            log_to_android(&mut env, "ok after term write");
        });
    }
}

fn prepare_config(
    app_dir: &Path,
    storage_root: &Path,
    config_content: &str,
) -> anyhow::Result<Config> {
    let AndroidConfig {
        use_keyring,
        always_exclude,
        mount_points,
        encryption_key,
        server_url,
        access_token,
        local_db_path,
        log_file,
        log_filter,
        warn_about_files_larger_than,
        enable_desktop_notifications,
        desktop_notification_interval,
    } = json5::from_str(config_content)?;

    if use_keyring.is_some() {
        bail!("use_keyring is not available on android");
    }
    if encryption_key.is_some() {
        bail!("encryption_key cannot be specified in config on android");
    }
    if access_token.is_some() {
        bail!("access_token cannot be specified in config on android");
    }
    if enable_desktop_notifications.is_some() {
        bail!("enable_desktop_notifications is not available on android");
    }
    if desktop_notification_interval.is_some() {
        bail!("desktop_notification_interval is not available on android");
    }

    Ok(Config {
        use_keyring: false,
        always_exclude,
        mount_points: mount_points
            .into_iter()
            .map(|mount_point| {
                Ok(MountPoint {
                    local_path: SanitizedLocalPath::new(
                        storage_root.join(&mount_point.local_path),
                    )?
                    .canonicalize()?,
                    archive_path: mount_point.archive_path,
                    exclude: mount_point.exclude,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        encryption_key: None,
        server_url,
        access_token: None,
        local_db_path: Some(
            app_dir.join(
                local_db_path
                    .as_deref()
                    .unwrap_or(Path::new("rammingen.db")),
            ),
        ),
        log_file: log_file.map(|log_file| app_dir.join(log_file)),
        log_filter,
        warn_about_files_larger_than,
        enable_desktop_notifications: false,
        desktop_notification_interval: default_desktop_notification_interval(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AndroidConfig {
    #[serde(default)]
    pub use_keyring: Option<bool>,
    pub always_exclude: Vec<Rule>,
    pub mount_points: Vec<AndroidMountPoint>,
    pub encryption_key: Option<EncryptionKey>,
    pub server_url: Url,
    pub access_token: Option<AccessToken>,
    #[serde(default)]
    pub local_db_path: Option<PathBuf>,
    #[serde(default)]
    pub log_file: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
    #[serde(default = "default_warn_about_files_larger_than")]
    pub warn_about_files_larger_than: Byte,
    pub enable_desktop_notifications: Option<bool>,
    #[serde(default, with = "humantime_serde")]
    pub desktop_notification_interval: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndroidMountPoint {
    pub local_path: PathBuf,
    #[serde(with = "serde_path_with_prefix")]
    pub archive_path: ArchivePath,
    #[serde(default)]
    pub exclude: Vec<Rule>,
}
