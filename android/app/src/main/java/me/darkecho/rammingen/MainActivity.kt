@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.app.AlertDialog
import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.view.Menu
import android.view.MenuInflater
import android.view.MenuItem
import android.widget.EditText
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBackIosNew
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Sync
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import me.darkecho.rammingen.ui.theme.RammingenTheme
import java.io.File


const val TAG = "rammingen"

class MainActivity : ComponentActivity() {
    var storageRoot: File? = null
    var fileBrowserDirectory: MutableState<File?> = mutableStateOf(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val externalDir = getExternalFilesDir(null)
        if (externalDir == null) {
            Toast.makeText(this, "External storage is currently unavailable", 10).show()
        } else {
            val storageRoot = File("${externalDir.absolutePath}/storage")
            if (!storageRoot.exists()) {
                if (storageRoot.mkdirs()) {
                    Log.i(TAG, "storageRoot created: $storageRoot")
                } else {
                    Toast.makeText(this, "Failed to create storage root dir", 10).show()
                }
            }
            this.storageRoot = storageRoot
            fileBrowserDirectory.value = storageRoot
        }

        this.onBackPressedDispatcher.addCallback(this, object: OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                backPressed()
            }
        })

        setContent {
            RammingenTheme {
                Scaffold(
                    topBar = {
                        TopAppBar(
                            title = {
                                Text("Rammingen file sync")
                            },
                            actions = {
                                IconButton(
                                    onClick = { runCommand("sync", "Sync") },
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
                                        text = { Text("Settings") },
                                        onClick = {
                                            expanded.value = false
                                            onSettings()
                                        }
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Dry run") },
                                        onClick = {
                                            expanded.value = false
                                            runCommand("dry-run", "Dry run")
                                        }
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Server status") },
                                        onClick = {
                                            expanded.value = false
                                            runCommand("status", "Server status")
                                        }
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Command line help") },
                                        onClick = {
                                            expanded.value = false
                                            runCommand("help", "Help")
                                        }
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Run custom command") },
                                        onClick = {
                                            expanded.value = false
                                            runCustomCommand()
                                        }
                                    )
                                }
                            },
                        )
                    },
                ) { innerPadding ->
                    Box(Modifier
                        .padding(innerPadding)
                        .padding(horizontal = 16.dp)) {
                        Files(
                            directory = fileBrowserDirectory,
                            storageRoot = storageRoot,
                            onFileClick = { f, share -> onFileClick(f, share) },
                            editFile = { f -> editFile(f) },
                        )
                    }
                }
            }
        }
    }

    fun backPressed() {
        val directoryValue = fileBrowserDirectory.value ?: return
        if (storageRoot?.absolutePath == directoryValue.absolutePath) {
            return
        }
        val parent = directoryValue.parentFile ?: return
        fileBrowserDirectory.value = parent
    }

    fun runCommand(command: String, title: String) {
        startActivity(
            Intent(this, RunActivity::class.java)
                .putExtra("command", command)
                .putExtra("title", title)
                .putExtra("storageRoot", storageRoot?.absolutePath ?: "")
        )
    }

    fun runCustomCommand() {
        val edit = EditText(this)
        AlertDialog.Builder(this)
            .setMessage("Input command")
            .setView(edit)
            .setPositiveButton("Run") { _, _ ->
                val text = edit.text.toString()
                runCommand(text, text)
            }
            .create()
            .show()
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

    fun onSettings() {
        val intent = Intent(this, SettingsActivity::class.java)
        startActivity(intent)
    }

    fun onFileClick(file: File, share: Boolean) {
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
            sendIntent.setDataAndType(fileUri, contentResolver.getType(fileUri))
            sendIntent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            sendIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            sendIntent.action = if (share) {
                Intent.ACTION_SEND
            } else {
                Intent.ACTION_VIEW
            }
            sendIntent.putExtra(Intent.EXTRA_STREAM, fileUri)
            startActivity(Intent.createChooser(sendIntent, null))
        }
    }

    fun editFile(file: File) {
        startActivity(
            Intent(this, TextEditorActivity::class.java)
                .putExtra("filePath", file.absolutePath)
        )
    }
}

@Composable
fun Files(
    directory: MutableState<File?>,
    storageRoot: File?,
    onFileClick: (File, Boolean) -> Unit,
    editFile: (File) -> Unit,
) {
    val directoryValue = directory.value ?: return
    val parent = if (storageRoot?.absolutePath != directoryValue.absolutePath) {
        directoryValue.parentFile
    } else {
        null
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .fillMaxHeight()
    ) {
        Row {
            IconButton(
                onClick = { if (parent != null) { onFileClick(parent, false) } },
                enabled = parent != null,
            ) {
                Icon(Icons.Default.ArrowBackIosNew, "")
            }
            Column {
                Text("Browsing directory:")
                Text(
                    text = if (storageRoot != null) {
                        if (directoryValue.absolutePath == storageRoot.absolutePath) {
                            "/"
                        } else {
                            directoryValue.absolutePath.substring(storageRoot.absolutePath.length)
                        }
                    } else {
                        directoryValue.absolutePath
                    },
                    fontWeight = FontWeight.Bold,
                )
            }
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight()
                .verticalScroll(rememberScrollState())
        ) {
            val entries = directoryValue.listFiles()
            if (entries != null) {
                for (entry in entries) {
                    val expanded = remember { mutableStateOf(false) }

                    Row(
                        modifier = Modifier.fillMaxWidth()
                            .padding(0.dp, 12.dp)
                            .combinedClickable(
                                onClick = { onFileClick(entry, false) },
                                onLongClick = {
                                    Log.d(TAG, "onLongClick")
                                    expanded.value = true
                                },
                                onLongClickLabel = stringResource(R.string.open_context_menu)
                            ),
                    ) {
                        Icon(
                            imageVector = if (entry.isDirectory) {
                                Icons.Default.Folder
                            } else {
                                Icons.Default.FileOpen
                            },
                            contentDescription = ""
                        )
                        Text(
                            text = entry.name,
                            modifier = Modifier.fillMaxWidth(),
                            textAlign = TextAlign.Left,
                        )
                        DropdownMenu(
                            expanded = expanded.value,
                            onDismissRequest = { expanded.value = false }
                        ) {
                            if (!entry.isDirectory) {
                                DropdownMenuItem(
                                    text = { Text("Share") },
                                    onClick = {
                                        expanded.value = false
                                        onFileClick(entry, true)
                                    }
                                )
                                DropdownMenuItem(
                                    text = { Text("Edit as plain text") },
                                    onClick = {
                                        expanded.value = false
                                        editFile(entry)
                                    }
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}