package com.rustyprint.android

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Bundle
import android.print.PrintManager
import android.provider.Settings
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : AppCompatActivity() {
    private val scope = CoroutineScope(Dispatchers.Main + Job())
    private lateinit var prefs: SharedPreferences
    private lateinit var statusDot: View
    private lateinit var tvStatus: TextView
    private lateinit var btnEnableService: Button
    private lateinit var etPin: EditText
    private lateinit var btnSavePin: Button
    private lateinit var tvPrintersList: TextView
    private lateinit var btnRefresh: TextView

    private val discoveredPrinters = mutableListOf<String>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        prefs = getSharedPreferences(RustyPrintService.PREFS_NAME, Context.MODE_PRIVATE)

        statusDot = findViewById(R.id.statusDot)
        tvStatus = findViewById(R.id.tvStatus)
        btnEnableService = findViewById(R.id.btnEnableService)
        etPin = findViewById(R.id.etPin)
        btnSavePin = findViewById(R.id.btnSavePin)
        tvPrintersList = findViewById(R.id.tvPrintersList)
        btnRefresh = findViewById(R.id.btnRefresh)

        // Load saved PIN
        val savedPin = prefs.getString(RustyPrintService.KEY_PIN, "") ?: ""
        etPin.setText(savedPin)

        btnSavePin.setOnClickListener {
            val pin = etPin.text.toString().trim()
            prefs.edit().putString(RustyPrintService.KEY_PIN, pin).apply()
            Toast.makeText(this, "PIN saved", Toast.LENGTH_SHORT).show()
        }

        btnEnableService.setOnClickListener {
            try {
                startActivity(Intent(Settings.ACTION_PRINT_SETTINGS))
            } catch (e: Exception) {
                Toast.makeText(this, "Open Android Settings > Connected Devices > Printing", Toast.LENGTH_LONG).show()
            }
        }

        btnRefresh.setOnClickListener {
            scanPrinters()
        }
    }

    override fun onResume() {
        super.onResume()
        updateServiceStatus()
        scanPrinters()
    }

    private fun updateServiceStatus() {
        val disabledServices = Settings.Secure.getString(contentResolver, "disabled_print_services") ?: ""
        val myService = ComponentName(this, RustyPrintService::class.java).flattenToString()
        val isExplicitlyDisabled = disabledServices.contains(myService) || disabledServices.contains(RustyPrintService::class.java.name)

        if (!isExplicitlyDisabled) {
            tvStatus.text = "Print Service is Active"
            tvStatus.setTextColor(ContextCompat.getColor(this, R.color.success))
            statusDot.backgroundTintList = ContextCompat.getColorStateList(this, R.color.success)
            btnEnableService.text = "Manage in Android Settings"
        } else {
            tvStatus.text = "Turn ON Rusty Print in Settings"
            tvStatus.setTextColor(ContextCompat.getColor(this, R.color.text_secondary))
            statusDot.backgroundTintList = ContextCompat.getColorStateList(this, R.color.text_muted)
            btnEnableService.text = "Turn ON in Android Settings"
        }
    }

    private fun scanPrinters() {
        tvPrintersList.text = "Scanning Wi-Fi network for office print servers..."
        discoveredPrinters.clear()

        val nsdManager = getSystemService(Context.NSD_SERVICE) as NsdManager

        val listener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String?, errorCode: Int) {}
            override fun onStopDiscoveryFailed(serviceType: String?, errorCode: Int) {}
            override fun onDiscoveryStarted(serviceType: String?) {}
            override fun onDiscoveryStopped(serviceType: String?) {}

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                try {
                    nsdManager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                        override fun onResolveFailed(serviceInfo: NsdServiceInfo?, errorCode: Int) {}
                        override fun onServiceResolved(resolved: NsdServiceInfo) {
                            val host = resolved.host?.hostAddress ?: return
                            val port = resolved.port
                            scope.launch {
                                val printers = ApiClient.fetchPrinters(host, port)
                                withContext(Dispatchers.Main) {
                                    for (p in printers) {
                                        val line = "• ${p.name} ($host)"
                                        if (!discoveredPrinters.contains(line)) {
                                            discoveredPrinters.add(line)
                                        }
                                    }
                                    if (discoveredPrinters.isNotEmpty()) {
                                        tvPrintersList.text = discoveredPrinters.joinToString("\n")
                                    } else {
                                        tvPrintersList.text = "Found print server at $host:$port (no printers connected)"
                                    }
                                }
                            }
                        }
                    })
                } catch (e: Exception) {}
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {}
        }

        try {
            nsdManager.discoverServices("_rustyprint._tcp", NsdManager.PROTOCOL_DNS_SD, listener)
            // Stop discovery after 5 seconds to preserve battery
            window.decorView.postDelayed({
                try {
                    nsdManager.stopServiceDiscovery(listener)
                } catch (e: Exception) {}
                if (discoveredPrinters.isEmpty()) {
                    tvPrintersList.text = "No printers discovered yet. Make sure you are on office Wi-Fi."
                }
            }, 5000)
        } catch (e: Exception) {
            tvPrintersList.text = "Discovery error: ${e.message}"
        }
    }
}
