@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.content.ActivityNotFoundException
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
const val STORAGE_DIR_NAME = "storage"

class MainActivity : ComponentActivity() {
    private val viewModel: FileBrowserViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val storageRoot = prepareStorageRoot()
        if (storageRoot != null) {
            viewModel.setStorageRoot(storageRoot.absolutePath)
            viewModel.setCurrentDir(storageRoot.absolutePath)
        }

        val backPressedCallback =
            object : OnBackPressedCallback(false) {
                override fun handleOnBackPressed() = viewModel.goToParentDir()
            }
        onBackPressedDispatcher.addCallback(this, backPressedCallback)

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collect { uiState ->
                    if (uiState.runCommandRequest != null) {
                        runCommand(uiState.runCommandRequest)
                    }
                    if (uiState.settingsRequest) {
                        goToSettings()
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

    fun prepareStorageRoot(): File? {
        val externalDir = getExternalFilesDir(null)
        if (externalDir == null) {
            Toast
                .makeText(
                    this,
                    R.string.external_storage_is_currently_unavailable,
                    Toast.LENGTH_LONG,
                ).show()
            return null
        }

        val storageRoot = File(externalDir.absolutePath, STORAGE_DIR_NAME)
        if (!storageRoot.exists()) {
            if (storageRoot.mkdirs()) {
                Log.i(TAG, "storageRoot created: $storageRoot")
            } else {
                Toast
                    .makeText(
                        this,
                        R.string.failed_to_create_storage_root_dir,
                        Toast.LENGTH_LONG,
                    ).show()
                return null
            }
        }

        return storageRoot
    }

    private fun runCommand(request: RunCommandRequest) {
        startActivity(RunActivity.createIntent(this, request))
    }

    private fun goToSettings() {
        val intent = Intent(this, SettingsActivity::class.java)
        startActivity(intent)
    }

    private fun runFileAction(request: FileActionRequest) {
        when (request.action) {
            FileAction.OPEN, FileAction.SHARE -> {
                val fileUri =
                    try {
                        FileProvider.getUriForFile(
                            this@MainActivity,
                            "${BuildConfig.APPLICATION_ID}.provider",
                            File(request.filePath),
                        )
                    } catch (e: IllegalArgumentException) {
                        Log.e(
                            TAG,
                            "The selected file can't be shared: $request.file: $e",
                        )
                        return
                    }

                val sendIntent = Intent()
                sendIntent.setDataAndType(
                    fileUri,
                    contentResolver.getType(fileUri) ?: "*/*",
                )
                sendIntent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                sendIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                sendIntent.action =
                    when (request.action) {
                        FileAction.OPEN -> {
                            Intent.ACTION_VIEW
                        }
                        FileAction.SHARE -> Intent.ACTION_SEND
                        else -> {
                            Log.e(TAG, "unreachable")
                            return
                        }
                    }
                sendIntent.putExtra(Intent.EXTRA_STREAM, fileUri)

                try {
                    startActivity(Intent.createChooser(sendIntent, null))
                } catch (_: ActivityNotFoundException) {
                    Toast
                        .makeText(
                            this,
                            R.string.failed_to_open_file,
                            Toast.LENGTH_SHORT,
                        ).show()
                }
            }
            FileAction.EDIT -> {
                startActivity(
                    TextEditorActivity.createIntent(this, request.filePath),
                )
            }
        }
    }
}
