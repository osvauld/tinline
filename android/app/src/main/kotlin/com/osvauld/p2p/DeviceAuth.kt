package com.osvauld.p2p

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal

/**
 * Asks for the phone's screen lock (PIN, pattern, password or biometrics) before the recovery phrase
 * is shown when no passphrase guards it. The framework BiometricPrompt, so no extra dependency.
 */
object DeviceAuth {
    /** False when there is no screen lock (or the system is too old to ask): nothing to check against. */
    fun available(c: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        val km = c.getSystemService(KeyguardManager::class.java)
        return km?.isDeviceSecure == true
    }

    /** Shows the system prompt; [onResult] gets true once the user passed it, false if they cancelled or it failed. */
    fun ask(a: Activity, title: String, onResult: (Boolean) -> Unit) {
        if (!available(a)) { onResult(true); return }
        val b = BiometricPrompt.Builder(a).setTitle(title)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            b.setAllowedAuthenticators(BiometricManager.Authenticators.DEVICE_CREDENTIAL)
        } else {
            @Suppress("DEPRECATION") b.setDeviceCredentialAllowed(true)
        }
        val cb = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult?) = onResult(true)
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) = onResult(false)
        }
        b.build().authenticate(CancellationSignal(), a.mainExecutor, cb)
    }
}
