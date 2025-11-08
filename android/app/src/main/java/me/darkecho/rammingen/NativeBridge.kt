package me.darkecho.rammingen

interface Receiver {
    fun onNativeBridgeLog(level: Int, text: String)
}
class NativeBridge {
    init {
        System.loadLibrary("rammingen_android")
    }

    @JvmName("add")
    external fun add(a: ULong, b: ULong, receiver: Receiver): ULong
}