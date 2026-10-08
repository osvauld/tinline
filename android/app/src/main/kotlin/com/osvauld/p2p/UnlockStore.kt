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
 * in AndroidKeyStore (no user-authentication requirement) and stored as `iv(12) || ciphertext`.
 *
 * One blob per account: filesDir/unlock/<did without "did:key:">.bin. The Keystore alias is shared
 * on purpose: it is only the wrapping key, each blob has its own IV, and one alias means a lock-screen
 * reset invalidates every blob at once (each then reads as [Loaded.Gone] and that account asks for
 * its passphrase or phrase). A blob of another account would not open this account's vault anyway
 * (the core answers WrongPassphrase). Never logs key material.
 */
object UnlockStore {
    private const val ALIAS = "p2p_unlock_v1"
    private const val LEGACY = "unlock.bin"
    private const val TAG = "UnlockStore"

    private fun dir(c: Context) = File(c.filesDir, "unlock")
    private fun name(did: String) = did.removePrefix("did:key:").filter { it.isLetterOrDigit() }.ifEmpty { "x" }
    private fun file(c: Context, did: String) = File(dir(c), name(did) + ".bin")

    fun exists(c: Context, did: String) = file(c, did).isFile

    /**
     * Upgrade from the single-key layout: the old unlock.bin belongs to the one account that existed,
     * which the core has just migrated to be the current one. Idempotent; never overwrites a per-account blob.
     */
    @Synchronized
    fun migrateLegacy(c: Context, currentDid: String?) {
        val old = File(c.filesDir, LEGACY)
        if (!old.isFile) return
        if (currentDid == null) return // nothing to attach it to yet; keep it
        val dst = file(c, currentDid)
        dir(c).mkdirs()
        if (dst.isFile) old.delete() else if (!old.renameTo(dst)) Log.w(TAG, "legacy key migration failed")
    }

    private fun keyStore() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    /**
     * The Keystore key. If the alias exists but cannot be used (UnrecoverableKeyException and
     * friends after a lock-screen reset or a restore), it is deleted and, when [create], regenerated.
     */
    private fun key(create: Boolean): SecretKey? {
        val ks = keyStore()
        val existing = try {
            ks.getKey(ALIAS, null) as? SecretKey
        } catch (e: java.security.UnrecoverableKeyException) {
            Log.w(TAG, "keystore alias unusable, dropping it")
            try { ks.deleteEntry(ALIAS) } catch (_: Exception) {}
            null
        }
        if (existing != null) return existing
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
    fun save(c: Context, did: String, secret: ByteArray): Boolean = try {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(true))
        val out = cipher.iv + cipher.doFinal(secret)
        dir(c).mkdirs()
        val dst = file(c, did)
        val tmp = File(dir(c), dst.name + ".tmp")
        tmp.outputStream().use { it.write(out); it.fd.sync() }
        if (!tmp.renameTo(dst)) throw java.io.IOException("rename failed")
        // Read it back: a key that cannot be opened again is no key (S1).
        (load(c, did) as? Loaded.Key)?.bytes?.fill(0) ?: throw java.io.IOException("read-back failed")
        true
    } catch (e: Exception) {
        Log.w(TAG, "save failed: ${e.javaClass.simpleName}")
        false
    }

    sealed interface Loaded {
        class Key(val bytes: ByteArray) : Loaded
        /** The wrapped key can never be opened again (corrupt, key gone or invalidated): forget it. */
        data object Gone : Loaded
        /** A Keystore/IO hiccup: keep unlock.bin and try again later. */
        data object Transient : Loaded
    }

    fun load(c: Context, did: String): Loaded = try {
        val raw = file(c, did).readBytes()
        if (raw.size < 12 + 16) throw javax.crypto.AEADBadTagException("short file")
        val k = key(false)
        if (k == null) Loaded.Gone
        else {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, k, GCMParameterSpec(128, raw, 0, 12))
            Loaded.Key(cipher.doFinal(raw, 12, raw.size - 12))
        }
    } catch (e: javax.crypto.AEADBadTagException) {
        Log.w(TAG, "load failed: bad tag"); Loaded.Gone
    } catch (e: android.security.keystore.KeyPermanentlyInvalidatedException) {
        Log.w(TAG, "load failed: key invalidated"); Loaded.Gone
    } catch (e: java.io.FileNotFoundException) {
        Loaded.Gone
    } catch (e: Exception) {
        Log.w(TAG, "load failed (transient): ${e.javaClass.simpleName}"); Loaded.Transient
    }

    fun clear(c: Context, did: String) {
        val f = file(c, did)
        f.delete()
        File(f.path + ".tmp").delete()
    }
}
