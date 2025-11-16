@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.launch
import me.darkecho.rammingen.ui.theme.RammingenTheme
import java.io.File


const val TAG = "rammingen"

class MainActivity : ComponentActivity() {
    private val viewModel: FileBrowserViewModel by viewModels()

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
            viewModel.setStorageRoot(storageRoot)
            viewModel.setCurrentDir(storageRoot)
        }

        val backPressedCallback = object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = viewModel.goToParentDir()
        }
        this.onBackPressedDispatcher.addCallback(this, backPressedCallback)

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collect { uiState ->
                    if (uiState.runCommandRequest != null) {
                        runCommand(uiState.runCommandRequest)
                    }
                    if (uiState.settingsRequest) {
                        onSettings()
                    }
                    if (uiState.fileActionRequest != null) {
                        runFileAction(uiState.fileActionRequest)
                    }
                    viewModel.clearRequests()
                    backPressedCallback.isEnabled = uiState.hasValidParent()
                }
            }
        }

        setContent {
            RammingenTheme {
                FileBrowserScreen(viewModel = viewModel)
            }
        }
    }

    fun runCommand(request: RunCommandRequest) {
        startActivity(
            Intent(this, RunActivity::class.java)
                .putExtra("command", request.command)
                .putExtra("title", request.title)
                .putExtra("storageRoot", request.storageRoot)
                .putExtra("currentDir", request.currentDir) // TODO: use for custom commands
        )
    }

    fun onSettings() {
        val intent = Intent(this, SettingsActivity::class.java)
        startActivity(intent)
    }

    fun runFileAction(request: FileActionRequest) {
        when(request.action) {
            FileAction.OPEN, FileAction.SHARE -> {
                val fileUri = try {
                    FileProvider.getUriForFile(
                        this@MainActivity,
                        applicationContext.applicationInfo.packageName + ".provider",
                        request.file,
                    )
                } catch (e: IllegalArgumentException) {
                    Log.e(TAG,
                        "The selected file can't be shared: $request.file: $e")
                    return
                }

                val sendIntent = Intent()
                sendIntent.setDataAndType(fileUri, contentResolver.getType(fileUri))
                sendIntent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                sendIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                sendIntent.action = when(request.action) {
                    FileAction.OPEN -> Intent.ACTION_VIEW
                    FileAction.SHARE -> Intent.ACTION_SEND
                    else -> {
                        Log.e(TAG, "unreachable")
                        return
                    }
                }
                sendIntent.putExtra(Intent.EXTRA_STREAM, fileUri)
                startActivity(Intent.createChooser(sendIntent, null))

            }
            FileAction.EDIT -> {
                startActivity(
                    Intent(this, TextEditorActivity::class.java)
                        .putExtra("filePath", request.file.absolutePath)
                )
            }
        }
    }
}
