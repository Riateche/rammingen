@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.util.Log
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.io.File

data class RunCommandRequest(
    val command: String,
    val title: String,
    val storageRoot: String,
    val currentDir: String,
)

enum class FileAction {
    OPEN, SHARE, EDIT,
}

data class FileActionRequest(
    val file: File,
    val action: FileAction,
)

data class FileBrowserState(
    val storageRoot: File? = null,
    val currentDir: File? = null,
    val contextMenuEntryPath: String? = null,
    val customCommand: String? = null,
    val runCommandRequest: RunCommandRequest? = null,
    val settingsRequest: Boolean = false,
    val fileActionRequest: FileActionRequest? = null,
) {
    fun isContextMenuOpen(file: File): Boolean {
        return file.absolutePath == contextMenuEntryPath
    }

    fun validParent(): File? {
        val currentDir = this.currentDir ?: return null
        val storageRoot = this.storageRoot ?: return null
        if (storageRoot.absolutePath == currentDir.absolutePath) {
            return null
        }
        return currentDir.parentFile
    }
    fun hasValidParent() = validParent() != null

    fun currentDirText(): String {
        val currentDir = this.currentDir ?: return ""
        val storageRoot = this.storageRoot ?: return ""
        return if (currentDir.absolutePath == storageRoot.absolutePath) {
            "/"
        } else {
            currentDir.absolutePath.substring(storageRoot.absolutePath.length)
        }
    }

    fun entries(): List<File> {
        val entries = currentDir?.listFiles() ?: emptyArray()
        return entries.sortedBy { it.name.lowercase() }
    }
}

class FileBrowserViewModel: ViewModel() {
    private val _uiState = MutableStateFlow(FileBrowserState())
    val uiState: StateFlow<FileBrowserState> = _uiState.asStateFlow()


    fun requestRunCommand(command: String, title: String) {
        _uiState.update { state ->
            val storageRoot = state.storageRoot ?: return
            val currentDirectory = state.currentDir ?: return
            state.copy(
                runCommandRequest = RunCommandRequest(
                    command = command,
                    title = title,
                    storageRoot = storageRoot.absolutePath,
                    currentDir = currentDirectory.absolutePath
                )
            )
        }
    }

    fun requestSettings() {
        _uiState.update { state ->
            state.copy(
                settingsRequest = true
            )
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

    fun openFile(file: File) {
        if (file.isDirectory) {
            _uiState.update { state ->
                state.copy(
                    currentDir = file
                )
            }
        } else {
            requestFileAction(file, FileAction.OPEN)
        }
    }

    fun requestFileAction(file: File, action: FileAction) {
        _uiState.update { state ->
            state.copy(
                fileActionRequest = FileActionRequest(file, action)
            )
        }
    }

    fun openContextMenu(file: File) {
        _uiState.update { state ->
            state.copy(
                contextMenuEntryPath = file.absolutePath
            )
        }
    }

    fun dismissContextMenu() {
        _uiState.update { state ->
            state.copy(
                contextMenuEntryPath = null
            )
        }
    }

    fun openCustomCommandDialog() {
        _uiState.update { state ->
            state.copy(
                customCommand = ""
            )
        }
    }

    fun dismissCustomCommandDialog() {
        _uiState.update { state ->
            state.copy(
                customCommand = null
            )
        }
    }

    fun setCustomCommandText(text: String) {
        _uiState.update { state ->
            state.copy(
                customCommand = text
            )
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

    fun setStorageRoot(file: File?) {
        _uiState.update { state ->
            state.copy(
                storageRoot = file
            )
        }
    }

    fun setCurrentDir(file: File?) {
        _uiState.update { state ->
            state.copy(
                currentDir = file
            )
        }
    }
}

@Composable
fun FileBrowserScreen(
    viewModel: FileBrowserViewModel
) {
    val state by viewModel.uiState.collectAsState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text("Rammingen file sync")
                },
                actions = {
                    IconButton(
                        onClick = { viewModel.requestRunCommand("sync", "Sync") },
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
                                viewModel.requestSettings()
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Dry run") },
                            onClick = {
                                expanded.value = false
                                viewModel.requestRunCommand("dry-run", "Dry run")
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Server status") },
                            onClick = {
                                expanded.value = false
                                viewModel.requestRunCommand("status", "Server status")
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Command line help") },
                            onClick = {
                                expanded.value = false
                                viewModel.requestRunCommand("help", "Help")
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Run custom command") },
                            onClick = {
                                expanded.value = false
                                viewModel.openCustomCommandDialog()
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
                currentDirText = state.currentDirText(),
                entries = state.entries(),
                hasValidParent = state.hasValidParent(),
                goToParentDir = { viewModel.goToParentDir() },
                openFile = { file -> viewModel.openFile(file) },
                requestFileAction = { file, action -> viewModel.requestFileAction(file, action) },
                isContextMenuOpen = { file -> state.isContextMenuOpen(file) },
                openContextMenu = { file -> viewModel.openContextMenu(file) },
                dismissContextMenu = { viewModel.dismissContextMenu() },
            )
            val customCommand = state.customCommand
            if (customCommand != null) {
                AlertDialog(
                    onDismissRequest = { viewModel.dismissCustomCommandDialog() },
                    title = { Text("Run custom command") },
                    text = {
                        val focusRequester = remember { FocusRequester() }
                        val keyboardController = LocalSoftwareKeyboardController.current
                        TextField(
                            value = customCommand,
                            singleLine = true,
                            onValueChange = { s -> viewModel.setCustomCommandText(s) },
                            modifier = Modifier
                                .focusRequester(focusRequester)
                                .onFocusChanged {
                                    if (it.isFocused) {
                                        keyboardController?.show()
                                    }
                                }
                        )
                        LaunchedEffect(Unit) {
                            focusRequester.requestFocus()
                        }
                    },
                    confirmButton = {
                        TextButton(
                            onClick = { viewModel.requestRunCustomCommand() }
                        ) {
                            Text("Run")
                        }
                    },
                    dismissButton = {
                        TextButton(
                            onClick = { viewModel.dismissCustomCommandDialog() }
                        ) {
                            Text("Cancel")
                        }
                    },
                )
            }
        }
    }
}


@Composable
fun Files(
    currentDirText: String,
    entries: List<File>,
    hasValidParent: Boolean,
    goToParentDir: () -> Unit,
    openFile: (File) -> Unit,
    requestFileAction: (File, FileAction) -> Unit,
    isContextMenuOpen: (File) -> Boolean,
    openContextMenu: (File) -> Unit,
    dismissContextMenu: () -> Unit
) {
    Column(modifier = Modifier.fillMaxSize()) {
        Row {
            IconButton(
                onClick = { goToParentDir() },
                enabled = hasValidParent,
            ) {
                Icon(Icons.Default.ArrowBackIosNew, "")
            }
            Column {
                Text("Browsing directory:")
                Text(
                    text = currentDirText,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
        ) {
            for (entry in entries) {
                Row(
                    modifier = Modifier.fillMaxWidth()
                        .padding(0.dp, 12.dp)
                        .combinedClickable(
                            onClick = { openFile(entry) },
                            onLongClick = {
                                Log.d(TAG, "onLongClick")
                                openContextMenu(entry)
                            },
                            onLongClickLabel = stringResource(R.string.open_context_menu)
                        ),
                ) {
                    if (entry.isDirectory) {
                        Icon(
                            imageVector = Icons.Default.Folder,
                            contentDescription = "folder",
                        )
                    } else {
                        Icon(
                            imageVector = Icons.Default.FileOpen,
                            contentDescription = "file",
                        )
                    }
                    Text(
                        text = entry.name,
                        modifier = Modifier.fillMaxWidth(),
                        textAlign = TextAlign.Left,
                    )
                    DropdownMenu(
                        expanded = isContextMenuOpen(entry),
                        onDismissRequest = dismissContextMenu
                    ) {
                        if (!entry.isDirectory) {
                            DropdownMenuItem(
                                text = { Text("Share") },
                                onClick = {
                                    dismissContextMenu()
                                    requestFileAction(entry, FileAction.SHARE)
                                }
                            )
                            DropdownMenuItem(
                                text = { Text("Edit as plain text") },
                                onClick = {
                                    dismissContextMenu()
                                    requestFileAction(entry, FileAction.EDIT)
                                }
                            )
                        }
                    }
                }
            }
        }
    }
}