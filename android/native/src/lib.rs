#![allow(clippy::missing_safety_doc)]

use {
    anyhow::{ensure, format_err},
    jni::{
        JNIEnv,
        objects::{GlobalRef, JObject, JValue},
        sys::{self, jboolean, jlong},
    },
    rammingen::{
        Secrets,
        cli::Command,
        config::Config,
        setup_logger,
        term::{Term, set_term},
    },
    scopeguard::defer,
    std::{any::Any, cell::Cell, panic, path::Path, ptr, sync::Once},
    tracing::{Level, error},
};

thread_local!(static JNI_ENV: Cell<*mut sys::JNIEnv> = const { Cell::new(ptr::null_mut()) });

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_me_darkecho_rammingen_NativeBridge_add(
    env: JNIEnv,
    _class: JObject,
    _a: jlong,
    _b: jlong,
    log_receiver: JObject,
) -> jboolean {
    let log_receiver = env
        .new_global_ref(log_receiver)
        .unwrap_or_else(|e| env.fatal_error(format!("new_global_ref failed: {e:?}")));

    JNI_ENV.with(|v| v.set(env.get_raw()));
    defer!(JNI_ENV.with(|v| v.set(ptr::null_mut())));

    let term = NativeBridgeTerm { log_receiver };
    set_term(Some(Box::new(term)));

    // let mut ctx = match Context::new(env, log_receiver.clone(), "rammingen_native") {
    //     Ok(ctx) => ctx,
    //     Err(code) => return code as i32,
    // };
    let r = run("", "", "");
    match r {
        Ok(()) => 1,
        Err(err) => {
            error!(?err);
            0
        }
    }
}

fn log_to_android(env: &mut JNIEnv<'_>, text: &str) {
    let log_class = env
        .find_class("android/util/Log")
        .unwrap_or_else(|e| env.fatal_error(format!("find_class(android/util/Log) failed: {e:?}")));
    let tag = env
        .new_string("rammingen_native")
        .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));
    let text = env
        .new_string(text)
        .unwrap_or_else(|e| env.fatal_error(format!("new_string failed: {e:?}")));

    env.call_static_method(
        &log_class,
        "d",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        &[JValue::Object(tag.as_ref()), JValue::Object(&text.into())],
    )
    .unwrap_or_else(|e| env.fatal_error(format!("Log.d failed: {e:?}")));
}

// struct Context<'a> {
//     env: JNIEnv<'a>,
//     log_receiver: GlobalRef,
//     log_class: JClass<'a>,
//     tag: JString<'a>,
// }

// impl<'a> Context<'a> {
//     fn new(mut env: JNIEnv<'a>, log_receiver: GlobalRef, tag: &str) -> Result<Self, ExitCode> {
//         let log_class = env
//             .find_class("android/util/Log")
//             .map_err(|_| ExitCode::FindClassLogFailed)?;
//         let tag = env.new_string(tag).map_err(|_| ExitCode::NewStringFailed)?;
//         Ok(Self {
//             log_class,
//             tag,
//             log_receiver,
//             env,
//         })
//     }

//     fn log(&mut self, level: Level, message: impl Display) {
//         let Ok(message) = self.env.new_string(message.to_string()) else {
//             std::process::exit(ExitCode::NewStringFailed as i32);
//         };
//         let r = self.env.call_method(
//             &self.log_receiver,
//             "onNativeBridgeLog",
//             "(ILjava/lang/String;)V",
//             &[JValue::Int(level_to_i32(level)), JValue::Object(&message)],
//         );

//         if let Err(error) = &r {
//             let Ok(error_message) = self
//                 .env
//                 .new_string(format!("call_method(onNativeBridgeLog) failed: {error:?}"))
//             else {
//                 std::process::exit(ExitCode::NewStringFailed as i32);
//             };
//             let r = self.env.call_static_method(
//                 &self.log_class,
//                 "e",
//                 "(Ljava/lang/String;Ljava/lang/String;)I",
//                 &[
//                     JValue::Object(self.tag.as_ref()),
//                     JValue::Object(&error_message.into()),
//                 ],
//             );
//             if r.is_err() {
//                 std::process::exit(ExitCode::CallUtilLogFailed as i32);
//             }

//             let r = self.env.call_static_method(
//                 &self.log_class,
//                 "e",
//                 "(Ljava/lang/String;Ljava/lang/String;)I",
//                 &[JValue::Object(self.tag.as_ref()), JValue::Object(&message)],
//             );
//             if r.is_err() {
//                 std::process::exit(ExitCode::CallUtilLogFailed as i32);
//             }
//         }
//     }
// }

fn run(
    config_path: impl AsRef<Path>,
    access_token: &str,
    encryption_key: &str,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut config: Config = json5::from_str(&fs_err::read_to_string(config_path.as_ref())?)?;

    static ONCE: Once = Once::new();
    let mut r = None;
    ONCE.call_once(|| {
        r = Some(setup_logger(
            config.log_file.clone(),
            config.log_filter.clone(),
        ));

        panic::set_hook(Box::new(|info| {
            error!(%info, "panic");
        }));
    });
    if let Some(r) = r {
        r?;
    }

    panic::catch_unwind(|| {
        ensure!(
            config.local_db_path.is_none(),
            "local_db_path cannot be set on Android"
        );
        config.local_db_path = Some("".into());
        //self.log(Level::INFO, "running...");
        runtime.block_on(rammingen::run(
            Command::Sync,
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
