package me.darkecho.rammingen

import android.app.AlertDialog
import android.content.Intent
import android.os.Bundle
import android.provider.DocumentsContract
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
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
import androidx.core.content.FileProvider
import me.darkecho.rammingen.ui.theme.RammingenTheme


const val TAG = "rammingen"

class MainActivity : ComponentActivity(), Receiver {
    var logsBuilder = AnnotatedString.Builder()
    var logs = mutableStateOf(logsBuilder.toAnnotatedString())
    var status = mutableStateOf("")
    var isRunning = mutableStateOf(false)
    var nativeBridge = NativeBridge()

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
        setContent {
            RammingenTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    Greeting(
                        modifier = Modifier.padding(innerPadding),
                        logs = logs,
                        status = status,
                        isRunning = isRunning,
                        onSync = { onSync() },
                        onSettings = { onSettings() },
                        onShowStorage = { onShowStorage() },
                    )
                }
            }
        }
    }

    fun onSync() {
        val dir = getExternalFilesDir(null)
        Log.d(TAG, "dir: $dir")
//        val persistedUris = contentResolver.persistedUriPermissions
//
//        for (perm in persistedUris) {
//            Log.d(TAG, "Persisted URI: ${perm.uri}, read=${perm.isReadPermission}, write=${perm.isWritePermission}")
//        }
//
//        val sm = getSystemService(STORAGE_SERVICE) as StorageManager
//        val intent = sm.primaryStorageVolume.createOpenDocumentTreeIntent()
//        intent.addCategory(Intent.CATEGORY_DEFAULT)
//        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or
//                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
//                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
//
//        openDocumentTreeLauncher.launch(intent)



//        if (checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)
//                == PackageManager.PERMISSION_GRANTED) {

        isRunning.value = true
        logsBuilder = AnnotatedString.Builder()
        logs.value = logsBuilder.toAnnotatedString()
        Thread {
            val isOk = nativeBridge.add(2u, 3u, this)
            isRunning.value = false
            runOnUiThread {
                if (isOk) {
                    logsBuilder.append("Operation finished successfully.\n")
                } else {
                    logsBuilder.append("Operation failed.\n")
                }
                logs.value = logsBuilder.toAnnotatedString()
            }
        }.start()
//        } else {
//            Log.i(TAG, "requestPermissions")
//            requestPermissions(arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE), 1)
//        }

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
            var color: Color
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
                    color = Color.Black
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
            logsBuilder.withStyle(SpanStyle(color)) {
                append(text + "\n")
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

@Composable
fun Greeting(
    logs: MutableState<AnnotatedString>,
    status: MutableState<String>,
    isRunning: MutableState<Boolean>,
    onSync: () -> Unit,
    onSettings: () -> Unit,
    onShowStorage: () -> Unit,
    modifier: Modifier = Modifier
) {
    Column {
        Text(
            text = "Rammingen file sync",
            modifier = modifier,
        )
        Button(
            onClick = { onSync() },
            enabled = !isRunning.value
        ) {
            Text("Sync")
        }
        Button(
            onClick = { onSettings() },
        ) {
            Text("Settings")
        }
        Button(
            onClick = { onShowStorage() },
        ) {
            Text("Show storage")
        }
        Text(
            text = status.value,
            modifier = modifier,
        )
        Text(
            text = logs.value,
            modifier = modifier
                .fillMaxWidth()
                .fillMaxHeight()
                .horizontalScroll(rememberScrollState())
                .verticalScroll(rememberScrollState()),
        )
    }
}

@Preview(showBackground = true)
@Composable
fun GreetingPreview() {
    RammingenTheme {
        Greeting(
            remember { mutableStateOf(AnnotatedString("logs\nlogs\nlogs")) },
            remember { mutableStateOf("status") },
            remember { mutableStateOf(false) },
            {},
            {},
            {},
        )
    }
}
