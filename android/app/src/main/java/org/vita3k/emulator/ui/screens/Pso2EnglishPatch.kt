package org.vita3k.emulator.ui.screens

// Nova3K: installs a fan translation for Phantasy Star Online 2 (PS Vita, PCSG00141) that the user
// already has as a file. Nothing of the translation is shipped with Nova3K: the English patch is the
// work of the Arks-Layer team (arks-layer.com), and this screen only copies the user's file to the
// place the game reads it from.
//
// The game loads every archive in app0:data/vita/patches (000.cpk ... 052.cpk on 6.31). A translation
// is one more archive there, named 999.cpk so it comes last and replaces the Japanese text files.

import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.vita3k.emulator.NativeLib
import org.vita3k.emulator.R
import java.io.File
import java.io.RandomAccessFile

object Pso2EnglishPatch {
    const val TITLE_ID = "PCSG00141"
    private const val PATCH_DIR = "ux0/app/$TITLE_ID/data/vita/patches"
    private const val FILE_NAME = "999.cpk"
    private const val MAX_SIZE = 1L shl 30

    sealed class Status {
        object GameMissing : Status()
        object NotInstalled : Status()
        data class Installed(val sizeBytes: Long, val patchDate: String?) : Status()
    }

    sealed class Result {
        data class Ok(val patchDate: String?) : Result()
        data class Error(val reason: Int) : Result() // string resource
    }

    private fun patchDir(): File = File(NativeLib.getCurrentEmulatorPath(), PATCH_DIR)

    fun status(): Status {
        val dir = patchDir()
        if (!dir.isDirectory) return Status.GameMissing
        val file = File(dir, FILE_NAME)
        if (!file.isFile) return Status.NotInstalled
        return Status.Installed(file.length(), findPatchDate(file))
    }

    fun install(context: Context, uri: Uri): Result {
        val dir = patchDir()
        if (!dir.isDirectory) return Result.Error(R.string.pso2_en_patch_error_no_game)
        val part = File(dir, "$FILE_NAME.part")
        try {
            val input = context.contentResolver.openInputStream(uri)
                ?: return Result.Error(R.string.pso2_en_patch_error_read)
            input.use { src ->
                part.outputStream().use { dst ->
                    val buffer = ByteArray(1 shl 16)
                    var total = 0L
                    while (true) {
                        val n = src.read(buffer)
                        if (n < 0) break
                        total += n
                        if (total > MAX_SIZE) {
                            part.delete()
                            return Result.Error(R.string.pso2_en_patch_error_not_cpk)
                        }
                        dst.write(buffer, 0, n)
                    }
                }
            }
            if (!looksLikeTextArchive(part)) {
                part.delete()
                return Result.Error(R.string.pso2_en_patch_error_not_cpk)
            }
            val target = File(dir, FILE_NAME)
            if (target.exists() && !target.delete()) {
                part.delete()
                return Result.Error(R.string.pso2_en_patch_error_write)
            }
            if (!part.renameTo(target)) {
                part.delete()
                return Result.Error(R.string.pso2_en_patch_error_write)
            }
            return Result.Ok(findPatchDate(target))
        } catch (e: Exception) {
            part.delete()
            return Result.Error(R.string.pso2_en_patch_error_write)
        }
    }

    fun remove(): Boolean {
        val file = File(patchDir(), FILE_NAME)
        return !file.exists() || file.delete()
    }

    // A CRI archive ("CPK " header) whose file table lists game text files (*.ice)
    private fun looksLikeTextArchive(file: File): Boolean {
        if (file.length() < 4096) return false
        RandomAccessFile(file, "r").use { raf ->
            val head = ByteArray(minOf(file.length(), 1L shl 20).toInt())
            raf.readFully(head)
            if (!(head[0] == 'C'.code.toByte() && head[1] == 'P'.code.toByte() && head[2] == 'K'.code.toByte() && head[3] == ' '.code.toByte()))
                return false
            return indexOf(head, "CpkTocInfo".toByteArray(), 0) >= 0 && indexOf(head, ".ice".toByteArray(), 0) >= 0
        }
    }

    // The Arks-Layer patch says "... EN Patch dated MM/DD/YYYY" in its welcome text (UTF-16)
    private fun findPatchDate(file: File): String? = try {
        val data = file.readBytes()
        val key = "Patch dated ".toByteArray(Charsets.UTF_16LE)
        val at = indexOf(data, key, 0)
        if (at < 0) null else {
            val start = at + key.size
            val end = minOf(start + 20, data.size)
            String(data, start, end - start, Charsets.UTF_16LE).takeWhile { it.isDigit() || it == '/' || it == '.' || it == '-' }
                .ifEmpty { null }
        }
    } catch (e: Throwable) {
        null
    }

    private fun indexOf(data: ByteArray, key: ByteArray, from: Int): Int {
        if (key.isEmpty()) return from
        var i = from
        val last = data.size - key.size
        while (i <= last) {
            if (data[i] == key[0]) {
                var j = 1
                while (j < key.size && data[i + j] == key[j]) j++
                if (j == key.size) return i
            }
            i++
        }
        return -1
    }
}

@Composable
fun Pso2EnglishPatchDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<Pso2EnglishPatch.Status?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun refresh() {
        scope.launch { status = withContext(Dispatchers.IO) { Pso2EnglishPatch.status() } }
    }
    LaunchedEffect(Unit) { refresh() }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        busy = true
        message = context.getString(R.string.pso2_en_patch_installing)
        scope.launch {
            val result = withContext(Dispatchers.IO) { Pso2EnglishPatch.install(context, uri) }
            message = when (result) {
                is Pso2EnglishPatch.Result.Ok -> context.getString(R.string.pso2_en_patch_done, result.patchDate ?: "?")
                is Pso2EnglishPatch.Result.Error -> context.getString(result.reason)
            }
            busy = false
            refresh()
        }
    }

    val current = status
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(stringResource(R.string.pso2_en_patch_title)) },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                Text(stringResource(R.string.pso2_en_patch_about), style = MaterialTheme.typography.bodyMedium)
                Spacer(Modifier.height(12.dp))
                Text(
                    when (current) {
                        null -> ""
                        Pso2EnglishPatch.Status.GameMissing -> stringResource(R.string.pso2_en_patch_status_no_game)
                        Pso2EnglishPatch.Status.NotInstalled -> stringResource(R.string.pso2_en_patch_status_none)
                        is Pso2EnglishPatch.Status.Installed -> stringResource(
                            R.string.pso2_en_patch_status_installed,
                            current.patchDate ?: "?", current.sizeBytes / (1024 * 1024)
                        )
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.primary
                )
                message?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it, style = MaterialTheme.typography.bodyMedium)
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && current != null && current != Pso2EnglishPatch.Status.GameMissing,
                onClick = { picker.launch(arrayOf("*/*")) }
            ) { Text(stringResource(R.string.pso2_en_patch_choose)) }
        },
        dismissButton = {
            Column {
                if (current is Pso2EnglishPatch.Status.Installed) {
                    TextButton(enabled = !busy, onClick = {
                        scope.launch {
                            val ok = withContext(Dispatchers.IO) { Pso2EnglishPatch.remove() }
                            message = context.getString(if (ok) R.string.pso2_en_patch_removed else R.string.pso2_en_patch_error_write)
                            refresh()
                        }
                    }) { Text(stringResource(R.string.pso2_en_patch_remove), color = MaterialTheme.colorScheme.error) }
                }
                TextButton(enabled = !busy, onClick = onDismiss) { Text(stringResource(R.string.pso2_en_patch_close)) }
            }
        }
    )
}
