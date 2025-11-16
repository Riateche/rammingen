@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.app.AlertDialog
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.colorspace.ColorSpaces
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import me.darkecho.rammingen.ui.theme.RammingenTheme

class RunActivity : ComponentActivity(), Receiver {
    var logsBuilder = AnnotatedString.Builder()
    val logs = mutableStateOf(logsBuilder.toAnnotatedString())
    val status = mutableStateOf("")
    val isRunning = mutableStateOf(false)
    val nativeBridge = NativeBridge()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val title = intent.getStringExtra("title") ?: return
        val command = intent.getStringExtra("command") ?: return
        val storageRoot = intent.getStringExtra("storageRoot") ?: return
        Log.d(TAG, "t2 $command, $title")

        enableEdgeToEdge()
        setContent {
            RammingenTheme {
                Scaffold(
                    topBar = {
                        TopAppBar(
                            title = {
                                Text(title)
                            },
                            navigationIcon = {
                                IconButton(onClick = {
                                    onBackPressedDispatcher.onBackPressed()
                                }) {
                                    Icon(
                                        imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                                        contentDescription = "Back"
                                    )
                                }
                            },
                        )
                    },
                ) { innerPadding ->
                    Box(Modifier
                        .padding(innerPadding)
                        .padding(horizontal = 16.dp)) {
                        Column {
                            if (!status.value.isEmpty()) {
                                Text(
                                    text = status.value,
                                )
                            }
                            if (!logs.value.isEmpty()) {
                                Text("Logs:")
                                Text(
                                    text = logs.value,
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .horizontalScroll(rememberScrollState())
                                        .verticalScroll(rememberScrollState()),
                                )
                            }
                        }
                    }
                }
            }
        }

        logsBuilder = AnnotatedString.Builder("")
        logs.value = logsBuilder.toAnnotatedString()

        val externalFilesDir = getExternalFilesDir(null)
        Log.d(TAG, "externalFilesDir: $externalFilesDir")
        if (externalFilesDir == null) {
            AlertDialog.Builder(this)
                .setMessage("Shared storage is not currently available")
                .create()
                .show()
            return
        }
        val dataStore = EncryptedPreferenceDataStore(this)
        val config = dataStore.getString("config", null)
        if (config.isNullOrEmpty()) {
            AlertDialog.Builder(this)
                .setMessage("Config not specified in settings")
                .create()
                .show()
            return
        }
        val accessToken = dataStore.getString("access_token", null)
        if (accessToken.isNullOrEmpty()) {
            AlertDialog.Builder(this)
                .setMessage("Access token not specified in settings")
                .create()
                .show()
            return
        }
        val encryptionKey = dataStore.getString("encryption_key", null)
        if (encryptionKey.isNullOrEmpty()) {
            AlertDialog.Builder(this)
                .setMessage("Encryption key not specified in settings")
                .create()
                .show()
            return
        }
        isRunning.value = true
        status.value = "Launching operation"
        Thread {
            val isOk = nativeBridge.run(
                externalFilesDir.absolutePath,
                storageRoot,
                config,
                accessToken,
                encryptionKey,
                command,
                this
            )
            runOnUiThread {
                isRunning.value = false
                if (isOk) {
                    status.value = "Operation finished successfully."
                } else {
                    status.value = "Operation failed."
                }
            }
        }.start()
    }

    override fun onNativeBridgeLog(level: Int, text: String) {
        runOnUiThread {
            var priority: Int
            var color: Color?
            when(level) {
                0 -> { // TRACE
                    priority = Log.VERBOSE
                    color = Color.Gray
                }
                1 -> { // DEBUG
                    priority = Log.VERBOSE
                    color = Color.Gray
                }
                2 -> { // INFO
                    priority = Log.INFO
                    color = null
                }
                3 -> { // WARN
                    priority = Log.WARN
                    color = Color(1f, 0.741f, 0f, 1f, ColorSpaces.Srgb)
                }
                else -> { // ERROR
                    priority = Log.ERROR
                    color = Color.Red
                }
            }
            Log.println(priority, "rammingen_native", text)
            if (color != null) {
                logsBuilder.withStyle(SpanStyle(color)) {
                    append(text + "\n")
                }
            } else {
                logsBuilder.append(text + "\n")
            }
            logs.value = logsBuilder.toAnnotatedString()
        }
    }

    override fun onNativeBridgeStatus(status: String) {
        runOnUiThread {
            Log.println(Log.INFO, "rammingen_native_status", status)
            this.status.value = status
        }
    }
}

