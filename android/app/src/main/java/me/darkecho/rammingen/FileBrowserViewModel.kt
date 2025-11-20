@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.lifecycle.ViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.io.File
import java.util.Locale

data class RunCommandRequest(
    val command: String,
    val title: String,
    val storageRoot: String,
    val currentDir: String,
)

enum class FileAction {
    OPEN,
    SHARE,
    EDIT,
}

data class FileActionRequest(
    val filePath: String,
    val action: FileAction,
)

data class FileBrowserState(
    val externalFileDir: String? = null,
    val storageRoot: String? = null,
    val currentDir: String? = null,
    val contextMenuEntryPath: String? = null,
    val customCommand: String? = null,
    val runCommandRequest: RunCommandRequest? = null,
    val settingsRequest: Boolean = false,
    val fileActionRequest: FileActionRequest? = null,
) {
    fun isContextMenuOpen(path: String): Boolean = path == contextMenuEntryPath

    fun validParent(): String? {
        val currentDir = this.currentDir ?: return null
        val storageRoot = this.storageRoot ?: return null
        if (storageRoot == currentDir) {
            return null
        }
        return File(currentDir).parent
    }

    fun hasValidParent() = validParent() != null

    fun currentDirText(): String {
        val currentDir = this.currentDir ?: return ""
        val storageRoot = this.storageRoot ?: return ""
        return currentDir
            .removePrefix(storageRoot)
            .ifEmpty { "/" }
    }

    fun entries(): List<File> {
        val currentDir = currentDir ?: return emptyList()
        val entries = File(currentDir).listFiles() ?: emptyArray()
        return entries.sortedBy { it.name.lowercase(Locale.getDefault()) }
    }
}

class FileBrowserViewModel : ViewModel() {
    private val _uiState = MutableStateFlow(FileBrowserState())
    val uiState: StateFlow<FileBrowserState> = _uiState.asStateFlow()

    fun requestRunCommand(
        command: String,
        title: String,
    ) {
        _uiState.update f@{ state ->
            val storageRoot = state.storageRoot ?: return@f state
            val currentDirectory = state.currentDir ?: return@f state
            state.copy(
                runCommandRequest =
                    RunCommandRequest(
                        command = command,
                        title = title,
                        storageRoot = storageRoot,
                        currentDir = currentDirectory,
                    ),
            )
        }
    }

    fun requestSettings() {
        _uiState.update { state ->
            state.copy(settingsRequest = true)
        }
    }

    fun goToParentDir() {
        _uiState.update { state ->
            val parent = state.validParent()
            if (parent == null) {
                state
            } else {
                state.copy(currentDir = parent)
            }
        }
    }

    fun openFile(path: String) {
        if (File(path).isDirectory) {
            _uiState.update { state ->
                state.copy(currentDir = path)
            }
        } else {
            requestFileAction(path, FileAction.OPEN)
        }
    }

    fun requestFileAction(
        filePath: String,
        action: FileAction,
    ) {
        _uiState.update { state ->
            state.copy(
                fileActionRequest = FileActionRequest(filePath, action),
            )
        }
    }

    fun openContextMenu(filePath: String) {
        _uiState.update { state ->
            state.copy(contextMenuEntryPath = filePath)
        }
    }

    fun dismissContextMenu() {
        _uiState.update { state ->
            state.copy(contextMenuEntryPath = null)
        }
    }

    fun openCustomCommandDialog() {
        _uiState.update { state ->
            state.copy(customCommand = "")
        }
    }

    fun dismissCustomCommandDialog() {
        _uiState.update { state ->
            state.copy(customCommand = null)
        }
    }

    fun setCustomCommandText(text: String) {
        _uiState.update { state ->
            state.copy(customCommand = text)
        }
    }

    fun requestRunCustomCommand() {
        val customCommand = _uiState.value.customCommand ?: return
        dismissCustomCommandDialog()
        requestRunCommand(customCommand, customCommand)
    }

    fun clearRequests() {
        _uiState.update { state ->
            state.copy(
                runCommandRequest = null,
                settingsRequest = false,
                fileActionRequest = null,
            )
        }
    }

    fun setStorageRoot(path: String?) {
        _uiState.update { state ->
            state.copy(storageRoot = path)
        }
    }

    fun setCurrentDir(path: String?) {
        _uiState.update { state ->
            state.copy(currentDir = path)
        }
    }
}
