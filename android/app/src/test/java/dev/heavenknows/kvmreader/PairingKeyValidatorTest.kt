package dev.heavenknows.kvmreader

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingKeyValidatorTest {
    @Test fun acceptsLegacyHexKey() {
        assertTrue(PairingKeyValidator.isValid("0123456789abcdef0123456789ABCDEF"))
    }
    @Test fun acceptsDesktopShortCodes() {
        assertTrue(PairingKeyValidator.isValid("A7B82C"))
        assertTrue(PairingKeyValidator.isValid("12345678"))
    }
    @Test fun pairingKeyDerivationMatchesDesktopCaseNormalization() {
        val upper = PairingKeyValidator.derivePsk("ABC234")
        val lower = PairingKeyValidator.derivePsk("abc234")
        assertTrue(upper.contentEquals(lower))
        assertTrue(upper.size == 32)
    }
    @Test fun rejectsInvalidKeys() {
        assertFalse(PairingKeyValidator.isValid("abc12"))
        assertFalse(PairingKeyValidator.isValid("0123456789abcdef0123456789abcdeg"))
        assertFalse(PairingKeyValidator.isValid("中文12345"))
        assertFalse(PairingKeyValidator.isValid("A7B8-2C"))
    }
}
