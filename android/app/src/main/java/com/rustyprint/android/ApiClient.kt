package com.rustyprint.android

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.util.concurrent.TimeUnit

data class DiscoveredPrinter(
    val id: String,
    val name: String,
    val serverHost: String,
    val serverPort: Int,
    val isDefault: Boolean
)

object ApiClient {
    private const val TAG = "RustyApiClient"
    private val client = OkHttpClient.Builder()
        .connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .build()

    suspend fun fetchPrinters(host: String, port: Int): List<DiscoveredPrinter> = withContext(Dispatchers.IO) {
        val url = "http://$host:$port/api/v1/status"
        val printers = mutableListOf<DiscoveredPrinter>()
        try {
            val req = Request.Builder().url(url).build()
            val resp = client.newCall(req).execute()
            if (resp.isSuccessful) {
                val body = resp.body?.string() ?: return@withContext emptyList()
                val json = JSONObject(body)
                val serverName = json.optString("server_name", "Office Printer")
                val activePrinter = json.optString("active_printer", "")
                val printersArr = json.optJSONArray("printers")

                if (printersArr != null && printersArr.length() > 0) {
                    for (i in 0 until printersArr.length()) {
                        val p = printersArr.getJSONObject(i)
                        val pName = p.getString("name")
                        val isDef = p.optBoolean("is_default", false)
                        val displayName = "$pName ($serverName)"
                        val pId = "${host}_${port}_${pName.replace(" ", "_")}"
                        printers.add(DiscoveredPrinter(pId, displayName, host, port, isDef))
                    }
                } else if (activePrinter.isNotEmpty()) {
                    val pId = "${host}_${port}_active"
                    printers.add(DiscoveredPrinter(pId, "$activePrinter ($serverName)", host, port, true))
                } else {
                    val pId = "${host}_${port}_default"
                    printers.add(DiscoveredPrinter(pId, serverName, host, port, true))
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to query status from $url: ${e.message}")
        }
        printers
    }

    suspend fun sendPrintJob(
        host: String,
        port: Int,
        printerName: String?,
        pin: String,
        copies: Int,
        colorMode: String,
        pdfBytes: ByteArray
    ): Pair<Boolean, String> = withContext(Dispatchers.IO) {
        val url = "http://$host:$port/api/v1/print"
        try {
            val fileBody = pdfBytes.toRequestBody("application/pdf".toMediaType())
            val builder = MultipartBody.Builder()
                .setType(MultipartBody.FORM)
                .addFormDataPart("file", "android_print_doc.pdf", fileBody)
                .addFormDataPart("pin", pin)
                .addFormDataPart("copies", copies.toString())
                .addFormDataPart("color_mode", colorMode)

            if (!printerName.isNullOrEmpty()) {
                builder.addFormDataPart("printer_name", printerName)
            }

            val req = Request.Builder()
                .url(url)
                .post(builder.build())
                .build()

            val resp = client.newCall(req).execute()
            val respBody = resp.body?.string() ?: ""
            if (resp.isSuccessful) {
                val json = JSONObject(respBody)
                val success = json.optBoolean("success", false)
                val msg = json.optString("message", "Print job sent")
                Pair(success, msg)
            } else {
                Pair(false, "Server error (${resp.code}): $respBody")
            }
        } catch (e: Exception) {
            Log.e(TAG, "Print job failed to $url", e)
            Pair(false, e.message ?: "Network error sending print job")
        }
    }
}
