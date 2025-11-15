@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.view.Menu
import android.view.MenuInflater
import android.view.MenuItem
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.colorspace.ColorSpaces
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import me.darkecho.rammingen.ui.theme.RammingenTheme
import java.io.File

const val TAG = "rammingen"

class MainActivity : ComponentActivity(), Receiver {
    var logsBuilder = AnnotatedString.Builder()
    var logs = mutableStateOf(logsBuilder.toAnnotatedString())
    var status = mutableStateOf("")
    var isRunning = mutableStateOf(false)
    var nativeBridge = NativeBridge()
    var fileBrowserDirectory: MutableState<File?> = mutableStateOf(null)

//    val openDocumentTreeLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
//        if (result.resultCode == RESULT_OK) {
//            val uri: Uri? = result.data?.data
//            if (uri != null) {
//                // Persist permission so we can use it later
//                contentResolver.takePersistableUriPermission(
//                    uri,
//                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
//                )
//
//                // Example: list files in that folder
//                val pickedDir = DocumentFile.fromTreeUri(this, uri)
//                pickedDir?.listFiles()?.forEach {
//                    println("Found file: ${it.name}")
//                }
//            }
//        }
//    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        fileBrowserDirectory.value = getExternalFilesDir(null)
        setContent {
            RammingenTheme {
                Scaffold(topBar = {
                    TopAppBar(
                        title = {
                            Text("Rammingen file sync")
                        },
                        actions = {
                            IconButton(
                                onClick = { onSync() },
                                enabled = !isRunning.value,
                            ) {
                                Icon(Icons.Default.Sync, contentDescription = "Sync")
                            }
                            val expanded = remember { mutableStateOf(false) }
                            IconButton(onClick = { expanded.value = !expanded.value }) {
                                 Icon(Icons.Default.MoreVert, contentDescription = "Menu")
                            }
                            DropdownMenu(
                                expanded = expanded.value,
                                onDismissRequest = { expanded.value = false }
                            ) {
                                DropdownMenuItem(
                                    text = { Text("Show storage") },
                                    onClick = { onShowStorage() }
                                )
                                DropdownMenuItem(
                                    text = { Text("Settings") },
                                    onClick = { onSettings() }
                                )
                            }
                        },
                    )
                },) { innerPadding ->
                    Box(Modifier.padding(innerPadding).padding(horizontal = 16.dp)) {
                        Greeting(
                            logs = logs,
                            status = status,
                            directory = fileBrowserDirectory,
                            onClick = { f -> onClick(f) },
                        )
                    }
                }
            }
        }
    }

    fun onSync() {
        logsBuilder = AnnotatedString.Builder("Logs:\n")
        logs.value = logsBuilder.toAnnotatedString()

        val dir = getExternalFilesDir(null)
        Log.d(TAG, "externalFilesDir: $dir")
        if (dir == null) {
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
                dir.absolutePath,
                config,
                accessToken,
                encryptionKey,
                "sync",
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

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        val inflater: MenuInflater = menuInflater
        inflater.inflate(R.menu.app_menu, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        Log.i(TAG, "onOptionsItemSelected: ${item.itemId}")
        // Handle item selection.
        return when (item.itemId) {
            R.id.dry_run -> {
                Log.i(TAG, "dry_run")
                true
            }
            R.id.server_status -> {
                Log.i(TAG, "server_status")
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    fun onShowStorage() {
        val dir = getExternalFilesDir(null)
        Log.d(TAG, "externalFilesDir: $dir")
        if (dir == null) {
            AlertDialog.Builder(this)
                .setMessage("Shared storage is not currently available")
                .create()
                .show()
            return
        }

        val sendIntent: Intent = Intent().apply {
            action = Intent.ACTION_SEND
            putExtra(Intent.EXTRA_TEXT, dir.absolutePath)
            type = "text/plain"
        }
        startActivity(Intent.createChooser(sendIntent, null))

    }

    fun onSettings() {
        val intent = Intent(this, SettingsActivity::class.java)
        startActivity(intent)
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

    fun onClick(file: File) {
        if (file.isDirectory) {
            fileBrowserDirectory.value = file
        } else {
            val fileUri = try {
                FileProvider.getUriForFile(
                    this@MainActivity,
                    applicationContext.applicationInfo.packageName + ".provider",
                    file
                )
            } catch (e: IllegalArgumentException) {
                Log.e(TAG,
                    "The selected file can't be shared: $file: $e")
                return
            }

            val sendIntent = Intent()
            Log.d(TAG, "fileUri=${fileUri}")
            Log.d(TAG, "type=${contentResolver.getType(fileUri)}")
            sendIntent.setDataAndType(fileUri, contentResolver.getType(fileUri))
            sendIntent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            sendIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            sendIntent.action = Intent.ACTION_VIEW
            sendIntent.putExtra(Intent.EXTRA_STREAM, fileUri)
            startActivity(Intent.createChooser(sendIntent, null))
        }
    }
}

@Composable
fun Greeting(
    logs: MutableState<AnnotatedString>,
    status: MutableState<String>,
    directory: MutableState<File?>,
    onClick: (File) -> Unit,
) {
    Column {
        if (!status.value.isEmpty()) {
            Text(
                text = status.value,
            )
        }
        Text(
            text = logs.value,
//            modifier = Modifier
//                .fillMaxWidth()
//                .fillMaxHeight()
//                .horizontalScroll(rememberScrollState())
//                .verticalScroll(rememberScrollState()),
        )
        Files(directory, onClick)
    }
}

@Composable
fun Files(
    directory: MutableState<File?>,
    onClick: (File) -> Unit,
) {
    Log.d(TAG, "ok0 ${directory.value}")
    val directoryValue = directory.value ?: return
    Log.d(TAG, "ok1 ${directoryValue.absolutePath}")
    Log.d(TAG, "ok2 ${directoryValue.listFiles()}")
    val parent = directoryValue.parentFile
    Text(text = directoryValue.absolutePath)
    Column {
        if (parent != null) {
            Button(
                onClick = {
                    onClick(parent);
                }
            ) {
                Text(text = "..")
            }
        }
        for (item in directoryValue.listFiles()) {
            Log.d(TAG, "ok3 ${item}")
            Log.d(TAG, "ok4 ${item.name}")
            Button(
                onClick = {
                    onClick(item);
                }
            ) {
                Text(text = item.name)
            }
        }
    }




}