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

    @JvmName("add")
    external fun add(a: ULong, b: ULong, receiver: Receiver): Boolean
}