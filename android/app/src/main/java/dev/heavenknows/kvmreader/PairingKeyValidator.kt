package dev.heavenknows.kvmreader
object PairingKeyValidator {
    fun isValid(value: String): Boolean {
        val key = value.trim()
        val legacyHex = key.length == 32 && key.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
        val shortCode = key.length in 6..16 && key.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' }
        return legacyHex || shortCode
    }
}
