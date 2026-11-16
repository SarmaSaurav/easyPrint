package com.rustyprint.android

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Handler
import android.os.Looper
import android.print.PrintAttributes
import android.print.PrinterCapabilitiesInfo
import android.print.PrinterId
import android.print.PrinterInfo
import android.printservice.PrintService
import android.printservice.PrinterDiscoverySession
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

class RustyDiscoverySession(private val printService: PrintService) : PrinterDiscoverySession() {
    private val scope = CoroutineScope(Dispatchers.Main + Job())
    private val nsdManager = printService.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val knownPrinters = mutableMapOf<String, DiscoveredPrinter>()
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private val handler = Handler(Looper.getMainLooper())

    override fun onStartPrinterDiscovery(priorityList: MutableList<PrinterId>) {
        Log.i(TAG, "Starting Rusty Print discovery session")
        startNsdDiscovery()
    }

    override fun onStopPrinterDiscovery() {
        Log.i(TAG, "Stopping Rusty Print discovery session")
        stopNsdDiscovery()
    }

    override fun onValidatePrinters(printerIds: MutableList<PrinterId>) {
        // Validate printers if needed
    }

    override fun onStartPrinterStateTracking(printerId: PrinterId) {
        // No-op
    }

    override fun onStopPrinterStateTracking(printerId: PrinterId) {
        // No-op
    }

    override fun onDestroy() {
        stopNsdDiscovery()
    }

    private fun startNsdDiscovery() {
        if (discoveryListener != null) return

        discoveryListener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String?, errorCode: Int) {
                Log.e(TAG, "Discovery start failed: $errorCode")
            }

            override fun onStopDiscoveryFailed(serviceType: String?, errorCode: Int) {
                Log.e(TAG, "Discovery stop failed: $errorCode")
            }

            override fun onDiscoveryStarted(serviceType: String?) {
                Log.i(TAG, "Discovery started for $serviceType")
            }

            override fun onDiscoveryStopped(serviceType: String?) {
                Log.i(TAG, "Discovery stopped")
            }

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                Log.i(TAG, "Service found: ${serviceInfo.serviceName}")
                try {
                    nsdManager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                        override fun onResolveFailed(serviceInfo: NsdServiceInfo?, errorCode: Int) {
                            Log.e(TAG, "Resolve failed: $errorCode")
                        }

                        override fun onServiceResolved(resolved: NsdServiceInfo) {
                            val host = resolved.host?.hostAddress ?: return
                            val port = resolved.port
                            Log.i(TAG, "Resolved service: $host:$port")
                            queryAndRegisterPrinters(host, port)
                        }
                    })
                } catch (e: Exception) {
                    Log.e(TAG, "Error resolving service", e)
                }
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                Log.i(TAG, "Service lost: ${serviceInfo.serviceName}")
            }
        }

        try {
            nsdManager.discoverServices("_rustyprint._tcp", NsdManager.PROTOCOL_DNS_SD, discoveryListener)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to call discoverServices", e)
        }
    }

    private fun stopNsdDiscovery() {
        discoveryListener?.let {
            try {
                nsdManager.stopServiceDiscovery(it)
            } catch (e: Exception) {
                Log.e(TAG, "Error stopping discovery", e)
            }
        }
        discoveryListener = null
    }

    fun queryAndRegisterPrinters(host: String, port: Int) {
        scope.launch {
            val printers = ApiClient.fetchPrinters(host, port)
            val printInfoList = mutableListOf<PrinterInfo>()

            for (p in printers) {
                knownPrinters[p.id] = p
                val printerId = printService.generatePrinterId(p.id)

                val capabilities = PrinterCapabilitiesInfo.Builder(printerId)
                    .addMediaSize(PrintAttributes.MediaSize.ISO_A4, true)
                    .addMediaSize(PrintAttributes.MediaSize.NA_LETTER, false)
                    .addResolution(PrintAttributes.Resolution("300dpi", "300 dpi", 300, 300), true)
                    .addResolution(PrintAttributes.Resolution("600dpi", "600 dpi", 600, 600), false)
                    .setColorModes(
                        PrintAttributes.COLOR_MODE_COLOR or PrintAttributes.COLOR_MODE_MONOCHROME,
                        PrintAttributes.COLOR_MODE_COLOR
                    )
                    .setMinMargins(PrintAttributes.Margins.NO_MARGINS)
                    .build()

                val info = PrinterInfo.Builder(printerId, p.name, PrinterInfo.STATUS_IDLE)
                    .setCapabilities(capabilities)
                    .setDescription("Office Wireless Printer on $host")
                    .build()

                printInfoList.add(info)
            }

            if (printInfoList.isNotEmpty()) {
                addPrinters(printInfoList)
                Log.i(TAG, "Registered ${printInfoList.size} printers with Android Print Spooler")
            }
        }
    }

    fun getPrinter(id: String): DiscoveredPrinter? = knownPrinters[id]

    companion object {
        private const val TAG = "RustyDiscovery"
    }
}
