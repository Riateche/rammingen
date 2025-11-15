@file:OptIn(ExperimentalMaterial3Api::class)

package me.darkecho.rammingen

import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Save
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.unit.dp
import me.darkecho.rammingen.ui.theme.RammingenTheme
import java.io.File
import java.io.FileOutputStream
import java.io.OutputStreamWriter
import java.nio.charset.StandardCharsets
import java.time.format.TextStyle


class TextEditorActivity: ComponentActivity() {
    val textState = TextFieldState("")
    var filePath: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val filePath = intent.getStringExtra("filePath") ?: return
        this.filePath = filePath
        val file = File(filePath)
        val text = file.readText(Charsets.UTF_8)
        textState.setTextAndPlaceCursorAtEnd(text)
        enableEdgeToEdge()

        setContent {
            RammingenTheme {
                Scaffold(
                    topBar = {
                        TopAppBar(
                            title = {
                                Text(file.name)
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
                            actions = {
                                IconButton(
                                    onClick = { save() },
                                ) {
                                    Icon(Icons.Default.Save, contentDescription = "Save")
                                }
                            }
                        )
                    },
                ) { innerPadding ->
                    Column(
                        Modifier
                            .padding(innerPadding)
                            .fillMaxWidth()
                            .fillMaxHeight()
                            .horizontalScroll(rememberScrollState())
                            .verticalScroll(rememberScrollState())
                    ) {
                        Column(
                            Modifier.padding(horizontal = 16.dp, vertical = 16.dp)
                        ) {
                            val localStyle = LocalTextStyle.current.copy(
                                color = LocalContentColor.current
                            )
                            BasicTextField(
                                textState,
                                textStyle = localStyle,
                                cursorBrush = SolidColor(LocalContentColor.current),
                            )
                        }
                    }
                }
            }
        }
    }

    fun save() {
        val filePath = filePath ?: return
        val writer = OutputStreamWriter(FileOutputStream(filePath), StandardCharsets.UTF_8)
        writer.use { writer ->
            writer.write(textState.text.toString())
        }
        Toast.makeText(this, "Saved", 5).show()
    }
}