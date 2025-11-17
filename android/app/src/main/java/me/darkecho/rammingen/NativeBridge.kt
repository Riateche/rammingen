package me.darkecho.rammingen

import android.system.Os

interface Receiver {
    fun onNativeBridgeLog(level: Int, text: String)
    fun onNativeBridgeStatus(status: String)
}
class NativeBridge {
    companion object {
        const val COMMAND_SYNC = "sync"
        const val COMMAND_DRY_RUN = "dry-run"
        const val COMMAND_SERVER_STATUS = "status"
        const val COMMAND_HELP = "help"
    }

    init {
        Os.setenv("RUST_BACKTRACE", "1", true)
        System.loadLibrary("rammingen_android")
    }

    @JvmName("run")
    external fun run(
        appDir: String,
        storageRoot: String,
        config: String,
        accessToken: String,
        encryptionKey: String,
        args: String,
        receiver: Receiver
    ): Boolean
}