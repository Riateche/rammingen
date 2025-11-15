@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.app.AlertDialog
import android.content.DialogInterface
import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.view.Menu
import android.view.MenuInflater
import android.view.MenuItem
import android.view.View
import android.widget.EditText
import android.widget.Toast
import androidx.activity.ComponentActivity
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
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import me.darkecho.rammingen.ui.theme.RammingenTheme
import java.io.File


const val TAG = "rammingen"

class MainActivity : ComponentActivity() {
    var fileBrowserDirectory: MutableState<File?> = mutableStateOf(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val externalDir = getExternalFilesDir(null)
        fileBrowserDirectory.value = externalDir
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
                            root = externalDir,
                            onFileClick = { f, share -> onFileClick(f, share) },
                        )
                    }
                }
            }
        }
    }

    fun runCommand(command: String, title: String) {
        startActivity(
            Intent(this, RunActivity::class.java)
                .putExtra("command", command)
                .putExtra("title", title)
        )
    }

    fun runCustomCommand() {
        val edit = EditText(this)
        AlertDialog.Builder(this)
            .setMessage("Input command")
            .setView(edit)
            .setPositiveButton("Run") { dialog, whichButton ->
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
            };
            sendIntent.putExtra(Intent.EXTRA_STREAM, fileUri)
            startActivity(Intent.createChooser(sendIntent, null))
        }
    }
}

@Composable
fun Files(
    directory: MutableState<File?>,
    root: File?,
    onFileClick: (File, Boolean) -> Unit,
) {
    Log.d(TAG, "ok0 ${directory.value}")
    val directoryValue = directory.value ?: return
    Log.d(TAG, "ok1 ${directoryValue.absolutePath}")
    Log.d(TAG, "ok2 ${directoryValue.listFiles()}")
    val parent = if (root?.absolutePath != directoryValue.absolutePath) {
        directoryValue.parentFile
    } else {
        null
    }
    Column {
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
                    text = if (root != null) {
                        if (directoryValue.absolutePath == root.absolutePath) {
                            "/"
                        } else {
                            directoryValue.absolutePath.substring(root.absolutePath.length)
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
                .horizontalScroll(rememberScrollState())
                .verticalScroll(rememberScrollState())
        ) {
            val entries = directoryValue.listFiles()
            if (entries != null) {
                for (item in entries) {
                    val expanded = remember { mutableStateOf(false) }

                    Row(
                        modifier = Modifier.fillMaxWidth()
                            .padding(0.dp, 8.dp)
                            .combinedClickable(
                                onClick = { onFileClick(item, false) },
                                onLongClick = {
                                    //haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    Log.d(TAG, "onLongClick")
                                    expanded.value = true
                                },
                                onLongClickLabel = stringResource(R.string.open_context_menu)
                            ),
                    ) {
                        Icon(
                            imageVector = if (item.isDirectory) {
                                Icons.Default.Folder
                            } else {
                                Icons.Default.FileOpen
                            },
                            contentDescription = ""
                        )
                        Text(
                            text = item.name,
                            modifier = Modifier.fillMaxWidth(),
                            textAlign = TextAlign.Left,
                        )
                        DropdownMenu(
                            expanded = expanded.value,
                            onDismissRequest = { expanded.value = false }
                        ) {
                            if (!item.isDirectory) {
                                DropdownMenuItem(
                                    text = { Text("Share") },
                                    onClick = {
                                        expanded.value = false
                                        onFileClick(item, true)
                                    }
                                )
                                DropdownMenuItem(
                                    text = { Text("Edit as plain text") },
                                    onClick = {
                                        expanded.value = false
                                        //...
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