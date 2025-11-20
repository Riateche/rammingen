@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.io.File

@Composable
fun FileBrowserScreen(viewModel: FileBrowserViewModel) {
    val state by viewModel.uiState.collectAsState()

    Scaffold(
        topBar = { TopBar(viewModel) },
    ) { innerPadding ->
        Box(Modifier.padding(innerPadding)) {
            Files(
                currentDirText = state.currentDirText(),
                entries = state.entries(),
                hasValidParent = state.hasValidParent(),
                goToParentDir = { viewModel.goToParentDir() },
                openFile = { path -> viewModel.openFile(path) },
                requestFileAction = { path, action -> viewModel.requestFileAction(path, action) },
                isContextMenuOpen = { path -> state.isContextMenuOpen(path) },
                openContextMenu = { path -> viewModel.openContextMenu(path) },
                dismissContextMenu = { viewModel.dismissContextMenu() },
            )
            val customCommand = state.customCommand
            if (customCommand != null) {
                RunCustomCommandDialog(
                    customCommandText = customCommand,
                    setCustomCommandText = { viewModel.setCustomCommandText(it) },
                    dismiss = { viewModel.dismissCustomCommandDialog() },
                    confirm = { viewModel.requestRunCustomCommand() },
                )
            }
        }
    }
}

@Composable
fun TopBar(viewModel: FileBrowserViewModel) {
    val resources = LocalResources.current

    TopAppBar(
        title = { Text(stringResource(R.string.long_app_title)) },
        actions = {
            IconButton(
                onClick = {
                    viewModel.requestRunCommand(
                        NativeBridge.COMMAND_SYNC,
                        resources.getString(R.string.sync),
                    )
                },
            ) {
                Icon(Icons.Default.Sync, contentDescription = stringResource(R.string.sync))
            }
            val (expanded, setExpanded) = remember { mutableStateOf(false) }
            IconButton(onClick = { setExpanded(!expanded) }) {
                Icon(
                    Icons.Default.MoreVert,
                    contentDescription = stringResource(R.string.menu),
                )
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { setExpanded(false) },
            ) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.settings)) },
                    onClick = {
                        setExpanded(false)
                        viewModel.requestSettings()
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.dry_run)) },
                    onClick = {
                        setExpanded(false)
                        viewModel.requestRunCommand(
                            NativeBridge.COMMAND_DRY_RUN,
                            resources.getString(R.string.dry_run),
                        )
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.server_status)) },
                    onClick = {
                        setExpanded(false)
                        viewModel.requestRunCommand(
                            NativeBridge.COMMAND_SERVER_STATUS,
                            resources.getString(R.string.server_status),
                        )
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.command_line_help)) },
                    onClick = {
                        setExpanded(false)
                        viewModel.requestRunCommand(
                            NativeBridge.COMMAND_HELP,
                            resources.getString(R.string.command_line_help),
                        )
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.run_custom_command)) },
                    onClick = {
                        setExpanded(false)
                        viewModel.openCustomCommandDialog()
                    },
                )
            }
        },
    )
}

@Composable
fun RunCustomCommandDialog(
    customCommandText: String,
    setCustomCommandText: (String) -> Unit,
    dismiss: () -> Unit,
    confirm: () -> Unit,
) {
    val focusRequester = remember { FocusRequester() }
    val keyboardController = LocalSoftwareKeyboardController.current

    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.run_custom_command)) },
        text = {
            TextField(
                value = customCommandText,
                singleLine = true,
                onValueChange = setCustomCommandText,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { confirm() }),
                modifier =
                    Modifier
                        .focusRequester(focusRequester)
                        .onFocusChanged {
                            if (it.isFocused) {
                                keyboardController?.show()
                            }
                        },
            )
            LaunchedEffect(Unit) {
                focusRequester.requestFocus()
            }
        },
        confirmButton = {
            TextButton(
                onClick = confirm,
            ) {
                Text(stringResource(R.string.run))
            }
        },
        dismissButton = {
            TextButton(
                onClick = dismiss,
            ) {
                Text(stringResource(R.string.cancel))
            }
        },
    )
}

@Composable
fun Files(
    currentDirText: String,
    entries: List<File>,
    hasValidParent: Boolean,
    goToParentDir: () -> Unit,
    openFile: (String) -> Unit,
    requestFileAction: (String, FileAction) -> Unit,
    isContextMenuOpen: (String) -> Boolean,
    openContextMenu: (String) -> Unit,
    dismissContextMenu: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxSize()) {
        Row {
            IconButton(
                onClick = { goToParentDir() },
                enabled = hasValidParent,
            ) {
                Icon(
                    imageVector = Icons.Default.ArrowBackIosNew,
                    contentDescription = stringResource(R.string.go_to_parent_directory),
                )
            }
            Column {
                Text(stringResource(R.string.browsing_directory))
                Text(
                    text = currentDirText,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState()),
        ) {
            for (entry in entries) {
                Row(
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .padding(0.dp, 12.dp)
                            .combinedClickable(
                                onClick = { openFile(entry.absolutePath) },
                                onLongClick = { openContextMenu(entry.absolutePath) },
                                onLongClickLabel = stringResource(R.string.open_context_menu),
                            ),
                ) {
                    if (entry.isDirectory) {
                        Icon(
                            imageVector = Icons.Default.Folder,
                            contentDescription = stringResource(R.string.folder),
                        )
                    } else {
                        Icon(
                            imageVector = Icons.Default.FileOpen,
                            contentDescription = stringResource(R.string.file),
                        )
                    }
                    Text(
                        text = entry.name,
                        modifier = Modifier.fillMaxWidth(),
                        textAlign = TextAlign.Left,
                    )
                    DropdownMenu(
                        expanded = isContextMenuOpen(entry.absolutePath),
                        onDismissRequest = dismissContextMenu,
                    ) {
                        if (!entry.isDirectory) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.share)) },
                                onClick = {
                                    dismissContextMenu()
                                    requestFileAction(entry.absolutePath, FileAction.SHARE)
                                },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.edit_as_plain_text)) },
                                onClick = {
                                    dismissContextMenu()
                                    requestFileAction(entry.absolutePath, FileAction.EDIT)
                                },
                            )
                        }
                    }
                }
            }
        }
    }
}
