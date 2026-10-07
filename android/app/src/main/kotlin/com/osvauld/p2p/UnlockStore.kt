package com.osvauld.p2p

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Remembers the core's vault data key on this device so the always-on service can unlock after a
 * reboot without the passphrase. The 32 bytes are wrapped with a non-exportable AES-256-GCM key
 * in AndroidKeyStore (no user-authentication requirement) and stored as `iv(12) || ciphertext`
 * in filesDir/unlock.bin. Never logs key material.
 */
object UnlockStore {
    private const val ALIAS = "p2p_unlock_v1"
    private const val FILE = "unlock.bin"
    private const val TAG = "UnlockStore"

    private fun file(c: Context) = File(c.filesDir, FILE)

    fun exists(c: Context) = file(c).isFile

    private fun keyStore() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun key(create: Boolean): SecretKey? {
        val ks = keyStore()
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        if (!create) return null
        val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        g.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return g.generateKey()
    }

    /** Wraps [secret] and writes unlock.bin atomically. Returns false on failure (nothing remembered). */
    fun save(c: Context, secret: ByteArray): Boolean = try {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(true))
        val out = cipher.iv + cipher.doFinal(secret)
        val tmp = File(c.filesDir, "$FILE.tmp")
        tmp.outputStream().use { it.write(out); it.fd.sync() }
        if (!tmp.renameTo(file(c))) throw java.io.IOException("rename failed")
        true
    } catch (e: Exception) {
        Log.w(TAG, "save failed: ${e.javaClass.simpleName}")
        false
    }

    /** The unwrapped key, or null if missing/invalid (the caller should then [clear]). */
    fun load(c: Context): ByteArray? = try {
        val raw = file(c).readBytes()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(false) ?: throw IllegalStateException("no keystore key"),
            GCMParameterSpec(128, raw, 0, 12))
        cipher.doFinal(raw, 12, raw.size - 12)
    } catch (e: Exception) {
        Log.w(TAG, "load failed: ${e.javaClass.simpleName}")
        null
    }

    fun clear(c: Context) {
        file(c).delete()
        File(c.filesDir, "$FILE.tmp").delete()
    }
}
