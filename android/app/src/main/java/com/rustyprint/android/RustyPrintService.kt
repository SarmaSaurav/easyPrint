package com.rustyprint.android

import android.content.Context
import android.content.SharedPreferences
import android.os.ParcelFileDescriptor
import android.print.PrintAttributes
import android.printservice.PrintJob
import android.printservice.PrintService
import android.printservice.PrinterDiscoverySession
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import java.io.FileInputStream

class RustyPrintService : PrintService() {
    private val scope = CoroutineScope(Dispatchers.IO + Job())
    private var currentSession: RustyDiscoverySession? = null
    private lateinit var prefs: SharedPreferences

    override fun onCreate() {
        super.onCreate()
        prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        Log.i(TAG, "RustyPrintService created")
    }

    override fun onCreatePrinterDiscoverySession(): PrinterDiscoverySession {
        Log.i(TAG, "Creating new RustyDiscoverySession")
        val session = RustyDiscoverySession(this)
        currentSession = session
        return session
    }

    override fun onRequestCancelPrintJob(printJob: PrintJob) {
        Log.i(TAG, "Print job cancel requested: ${printJob.id}")
        printJob.cancel()
    }

    override fun onPrintJobQueued(printJob: PrintJob) {
        val printerLocalId = printJob.info.printerId?.localId ?: "default"
        Log.i(TAG, "Print job queued: ${printJob.id} on printer $printerLocalId")

        if (printJob.isQueued) {
            printJob.start()
        }

        scope.launch {
            try {
                val pfd: ParcelFileDescriptor? = printJob.document.data
                if (pfd == null) {
                    printJob.fail("No document data provided by Android print framework")
                    return@launch
                }

                val pdfBytes = FileInputStream(pfd.fileDescriptor).use { input ->
                    input.readBytes()
                }

                if (pdfBytes.isEmpty()) {
                    printJob.fail("Document data was empty")
                    return@launch
                }

                val discovered = currentSession?.getPrinter(printerLocalId)

                // If not found in current session memory, parse from ID format: host_port_printerName
                val (host, port, printerName) = if (discovered != null) {
                    Triple(discovered.serverHost, discovered.serverPort, discovered.name)
                } else {
                    val parts = printerLocalId.split("_")
                    if (parts.size >= 3) {
                        val h = parts[0]
                        val pt = parts[1].toIntOrNull() ?: 8989
                        val pn = parts.drop(2).joinToString(" ")
                        Triple(h, pt, pn)
                    } else {
                        Triple("127.0.0.1", 8989, null)
                    }
                }

                val copies = printJob.info.copies.coerceAtLeast(1)
                val colorMode = if (printJob.info.attributes.colorMode == PrintAttributes.COLOR_MODE_MONOCHROME) {
                    "Grayscale"
                } else {
                    "Color"
                }

                val savedPin = prefs.getString(KEY_PIN, "") ?: ""

                Log.i(TAG, "Sending ${pdfBytes.size} bytes to $host:$port ($printerName), copies: $copies, color: $colorMode")

                val (success, message) = ApiClient.sendPrintJob(
                    host = host,
                    port = port,
                    printerName = printerName,
                    pin = savedPin,
                    copies = copies,
                    colorMode = colorMode,
                    pdfBytes = pdfBytes
                )

                if (success) {
                    Log.i(TAG, "Print job succeeded: $message")
                    printJob.complete()
                } else {
                    Log.e(TAG, "Print job failed: $message")
                    printJob.fail("Rusty Print error: $message")
                }

            } catch (e: Exception) {
                Log.e(TAG, "Exception during print job processing", e)
                printJob.fail("Error: ${e.message}")
            }
        }
    }

    companion object {
        private const val TAG = "RustyPrintService"
        const val PREFS_NAME = "rusty_print_prefs"
        const val KEY_PIN = "pin"
    }
}
