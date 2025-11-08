#![allow(clippy::missing_safety_doc)]

use {
    jni::{
        JNIEnv,
        objects::{JClass, JObject, JString, JValue},
        sys::jlong,
    },
    std::{thread::sleep, time::Duration},
    tracing::Level,
};

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_me_darkecho_rammingen_NativeBridge_add(
    env: JNIEnv,
    _class: JObject,
    _a: jlong,
    _b: jlong,
    receiver: JObject,
) -> jlong {
    let mut logger = Context::new(env, "rammingen_rust").unwrap();
    logger.d("rust logger ok").unwrap();
    for i in 0..10 {
        for level in [
            Level::TRACE,
            Level::DEBUG,
            Level::INFO,
            Level::WARN,
            Level::ERROR,
        ] {
            let r = logger.env.call_method(
                &receiver,
                "onNativeBridgeLog",
                "(ILjava/lang/String;)V",
                &[
                    JValue::Int(level_to_i32(level)),
                    JValue::Object(
                        &logger
                            .env
                            .new_string(format!("test{i} {}", level.as_str()))
                            .unwrap()
                            .into(),
                    ),
                ],
            );

            if let Err(err) = &r {
                logger.d(format!("error: {err:?}")).unwrap();
            }
        }
        sleep(Duration::from_millis(500));
    }
    1
}

struct Context<'a> {
    /// JNI environment.
    env: JNIEnv<'a>,
    /// Reference to the android.util.Log class.
    log_class: JClass<'a>,
    /// Tag for log messages.
    tag: JString<'a>,
}

impl<'a> Context<'a> {
    pub fn new(mut env: JNIEnv<'a>, tag: &str) -> Result<Self, jni::errors::Error> {
        Ok(Self {
            log_class: env.find_class("android/util/Log")?,
            tag: env.new_string(tag)?,
            env,
        })
    }

    /// Prints a message at the debug level.
    pub fn d(&mut self, message: impl AsRef<str>) -> Result<(), jni::errors::Error> {
        self.env.call_static_method(
            &self.log_class,
            "d",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValue::Object(self.tag.as_ref()),
                JValue::Object(&JObject::from(self.env.new_string(message)?)),
            ],
        )?;
        Ok(())
    }
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
