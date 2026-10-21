package dev.heavenknows.kvmreader

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicReference

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
    @Test fun noiseHandshakeAndEncryptedFramesRoundTrip() {
        val server = ServerSocket(0)
        val received = AtomicReference<ByteArray?>()
        val responder = Thread {
            server.accept().use { socket ->
                NoiseSession.accept(socket, "ABC234").use { session ->
                    received.set(session.receive())
                    session.send("ack".toByteArray(Charsets.UTF_8))
                }
            }
            server.close()
        }
        responder.start()
        Socket("127.0.0.1", server.localPort).use { socket ->
            NoiseSession.connect(socket, "abc234").use { session ->
                session.send("ping".toByteArray(Charsets.UTF_8))
                assertArrayEquals("ack".toByteArray(Charsets.UTF_8), session.receive())
            }
        }
        responder.join(5_000)
        assertArrayEquals("ping".toByteArray(Charsets.UTF_8), received.get())
    }

    @Test fun rejectsInvalidKeys() {
        assertFalse(PairingKeyValidator.isValid("abc12"))
        assertFalse(PairingKeyValidator.isValid("0123456789abcdef0123456789abcdeg"))
        assertFalse(PairingKeyValidator.isValid("中文12345"))
        assertFalse(PairingKeyValidator.isValid("A7B8-2C"))
    }
}
