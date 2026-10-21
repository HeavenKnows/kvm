package dev.heavenknows.kvmreader

import java.security.MessageDigest
import java.util.Locale

object PairingKeyValidator {
    fun isValid(value: String): Boolean {
        val key = value
        val legacyHex = key.length == 32 && key.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
        val shortCode = key.length in 6..16 && key.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' }
        return legacyHex || shortCode
    }

    /**
     * Keep byte-for-byte compatibility with src/wire.rs::handshake:
     * lowercase the pairing secret, UTF-8 encode, then SHA-256 to 32 bytes.
     */
    fun derivePsk(value: String): ByteArray {
        require(isValid(value)) { "invalid pairing key" }
        val normalized = value.lowercase(Locale.ROOT).toByteArray(Charsets.UTF_8)
        return MessageDigest.getInstance("SHA-256").digest(normalized)
    }
}

