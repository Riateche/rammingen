package me.darkecho.rammingen

import android.system.Os

interface Receiver {
    fun onNativeBridgeLog(level: Int, text: String)
    fun onNativeBridgeStatus(status: String)
}
class NativeBridge {
    init {
        Os.setenv("RUST_BACKTRACE", "1", true)
        System.loadLibrary("rammingen_android")
    }

    @JvmName("run")
    external fun run(
        appDir: String,
        config: String,
        accessToken: String,
        encryptionKey: String,
        args: String,
        receiver: Receiver
    ): Boolean
}