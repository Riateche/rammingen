package me.darkecho.rammingen

import android.app.Application as AndroidApplication

class Application : AndroidApplication() {
    override fun onCreate() {
        super.onCreate()
        com.google.crypto.tink.config.TinkConfig
            .register()
    }
}
