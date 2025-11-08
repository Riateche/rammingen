package me.darkecho.rammingen

import android.os.Bundle
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
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.colorspace.ColorSpaces
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.tooling.preview.Preview
import me.darkecho.rammingen.ui.theme.RammingenTheme

const val TAG = "rammingen"

class MainActivity : ComponentActivity(), Receiver {
    var logsBuilder = AnnotatedString.Builder()
    var logs = mutableStateOf(logsBuilder.toAnnotatedString())
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.i(TAG, "ok1")
        val rust = NativeBridge()
        Log.i(TAG, "ok2")

        Thread {
            val output = rust.add(2u, 3u, this)
            Log.i(TAG, "ok3 $output")
            // your code here
        }.start()


        enableEdgeToEdge()
        setContent {
            RammingenTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    Greeting(
                        name = "Android",
                        modifier = Modifier.padding(innerPadding),
                        logs = logs,
                    )
                }
            }
        }
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
}

@Composable
fun Greeting(name: String, logs: MutableState<AnnotatedString>, modifier: Modifier = Modifier) {
    Log.i(TAG, "Greeting logs: ${logs.value}")
    Column {
        Text(
            text = "Hello $name!",
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
        Greeting("Android", mutableStateOf(AnnotatedString("logs\nlogs\nlogs")))
    }
}