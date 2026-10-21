package dev.heavenknows.kvmreader

import java.security.MessageDigest
import java.util.Locale
import com.southernstorm.noise.protocol.CipherState
import com.southernstorm.noise.protocol.HandshakeState
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.Closeable
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.Socket

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


/**
 * Noise transport shared with the desktop protocol. This class only implements
 * authenticated framing; application messages remain owned by the wire adapter.
 */
class NoiseSession private constructor(
    private val socket: Socket,
    private val input: DataInputStream,
    private val output: DataOutputStream,
    private val sender: CipherState,
    private val receiver: CipherState
) : Closeable {
    @Synchronized
    fun send(plaintext: ByteArray) {
        require(plaintext.isNotEmpty() && plaintext.size <= MAX_FRAME - TAG_BYTES) {
            "invalid plaintext frame size"
        }
        val encrypted = ByteArray(plaintext.size + TAG_BYTES)
        val count = sender.encryptWithAd(null, plaintext, 0, encrypted, 0, plaintext.size)
        require(count in 1..MAX_FRAME) { "invalid encrypted frame size" }
        writeFrame(output, encrypted.copyOf(count))
    }

    @Synchronized
    fun receive(): ByteArray {
        val encrypted = readFrame(input)
        val plaintext = ByteArray(MAX_FRAME)
        val count = receiver.decryptWithAd(null, encrypted, 0, plaintext, 0, encrypted.size)
        require(count > 0) { "empty decrypted frame" }
        return plaintext.copyOf(count)
    }

    override fun close() {
        try { sender.destroy() } catch (_: Exception) { }
        try { receiver.destroy() } catch (_: Exception) { }
        try { socket.close() } catch (_: Exception) { }
    }

    companion object {
        const val MAX_FRAME = 60_000
        private const val TAG_BYTES = 16
        private const val PROTOCOL = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s"
        private val EMPTY = ByteArray(0)

        fun accept(socket: Socket, secret: String): NoiseSession = open(socket, secret, false)
        fun connect(socket: Socket, secret: String): NoiseSession = open(socket, secret, true)

        private fun open(socket: Socket, secret: String, initiator: Boolean): NoiseSession {
            require(PairingKeyValidator.isValid(secret)) { "invalid pairing key" }
            socket.tcpNoDelay = true
            socket.soTimeout = 4_000
            val input = DataInputStream(BufferedInputStream(socket.getInputStream()))
            val output = DataOutputStream(BufferedOutputStream(socket.getOutputStream()))
            val psk = PairingKeyValidator.derivePsk(secret)
            val handshake = HandshakeState(
                PROTOCOL,
                if (initiator) HandshakeState.INITIATOR else HandshakeState.RESPONDER
            )
            handshake.setPreSharedKey(psk, 0, psk.size)
            handshake.start()
            var pair: com.southernstorm.noise.protocol.CipherStatePair? = null
            try {
                while (pair == null) {
                    when (handshake.action) {
                        HandshakeState.WRITE_MESSAGE -> {
                            val message = ByteArray(MAX_FRAME)
                            val count = handshake.writeMessage(message, 0, EMPTY, 0, 0)
                            writeFrame(output, message.copyOf(count))
                        }
                        HandshakeState.READ_MESSAGE -> {
                            val message = readFrame(input)
                            val payload = ByteArray(MAX_FRAME)
                            handshake.readMessage(message, 0, message.size, payload, 0)
                        }
                        HandshakeState.SPLIT -> pair = handshake.split()
                        HandshakeState.FAILED -> error("Noise handshake failed")
                        else -> error("unexpected Noise handshake action: ${handshake.action}")
                    }
                }
                socket.soTimeout = 0
                val ready = pair ?: error("Noise handshake did not complete")
                return NoiseSession(socket, input, output, ready.sender, ready.receiver)
            } catch (error: Exception) {
                try { socket.close() } catch (_: Exception) { }
                throw error
            } finally {
                try { handshake.destroy() } catch (_: Exception) { }
                psk.fill(0)
            }
        }

        private fun writeFrame(output: DataOutputStream, bytes: ByteArray) {
            require(bytes.isNotEmpty() && bytes.size <= MAX_FRAME) { "invalid frame size" }
            output.writeShort(bytes.size)
            output.write(bytes)
            output.flush()
        }

        private fun readFrame(input: DataInputStream): ByteArray {
            val size = input.readUnsignedShort()
            require(size in 1..MAX_FRAME) { "invalid frame size: $size" }
            return ByteArray(size).also { input.readFully(it) }
        }
    }
}
