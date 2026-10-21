package dev.heavenknows.kvmreader

import org.msgpack.core.MessagePack
import org.msgpack.value.Value

/** The compact rmp-serde wire representation used by src/wire.rs. */
sealed class WireMessage {
    data class Hello(
        val os: String,
        val width: Int,
        val height: Int,
        val displays: List<Display>
    ) : WireMessage()
    data class Display(val x: Int, val y: Int, val width: Int, val height: Int)
    data class Enter(val x: Int, val y: Int, val side: String, val remoteDisplay: Int) : WireMessage()
    data class EnterAck(val accepted: Boolean) : WireMessage()
    object Back : WireMessage()
    data class Input(
        val kind: String,
        val value: String? = null,
        val x: Double? = null,
        val y: Double? = null
    ) : WireMessage()
    data class MoveBy(val dx: Int, val dy: Int) : WireMessage()
    data class Scroll(val dx: Int, val dy: Int) : WireMessage()
    data class Shortcut(val modifiers: Int, val key: String) : WireMessage()
    data class Ping(val tick: Long) : WireMessage()
    data class Pong(val tick: Long) : WireMessage()
    object ClipboardRequest : WireMessage()
    data class ClipboardBegin(val bytes: Int) : WireMessage()
    data class ClipboardChunk(val bytes: ByteArray) : WireMessage()
    object ClipboardEnd : WireMessage()
}

object MessagePackWireCodec {
    fun encode(message: WireMessage): ByteArray {
        val packer = MessagePack.newDefaultBufferPacker()
        when (message) {
            is WireMessage.Hello -> {
                packer.packMapHeader(1).packString("Hello")
                packer.packArrayHeader(3).packString(message.os)
                packer.packArrayHeader(2).packInt(message.width).packInt(message.height)
                packer.packArrayHeader(message.displays.size)
                message.displays.forEach {
                    packer.packArrayHeader(4).packInt(it.x).packInt(it.y)
                        .packInt(it.width).packInt(it.height)
                }
            }
            is WireMessage.Enter -> {
                packer.packMapHeader(1).packString("Enter")
                packer.packArrayHeader(4).packInt(message.x).packInt(message.y)
                    .packString(message.side).packInt(message.remoteDisplay)
            }
            is WireMessage.EnterAck -> {
                packer.packMapHeader(1).packString("EnterAck")
                packer.packArrayHeader(1).packBoolean(message.accepted)
            }
            WireMessage.Back -> packer.packString("Back")
            is WireMessage.MoveBy -> {
                packer.packMapHeader(1).packString("MoveBy")
                packer.packArrayHeader(2).packInt(message.dx).packInt(message.dy)
            }
            is WireMessage.Scroll -> {
                packer.packMapHeader(1).packString("Scroll")
                packer.packArrayHeader(2).packInt(message.dx).packInt(message.dy)
            }
            is WireMessage.Ping -> packer.packMapHeader(1).packString("Ping").packLong(message.tick)
            is WireMessage.Pong -> packer.packMapHeader(1).packString("Pong").packLong(message.tick)
            WireMessage.ClipboardRequest -> packer.packString("ClipboardRequest")
            is WireMessage.ClipboardBegin -> {
                packer.packMapHeader(1).packString("ClipboardBegin")
                packer.packArrayHeader(1).packInt(message.bytes)
            }
            is WireMessage.ClipboardChunk -> {
                packer.packMapHeader(1).packString("ClipboardChunk")
                packer.packArrayHeader(message.bytes.size)
                message.bytes.forEach { packer.packInt(it.toInt() and 0xff) }
            }
            WireMessage.ClipboardEnd -> packer.packString("ClipboardEnd")
            is WireMessage.Input -> {
                packer.packMapHeader(1).packString("Input")
                when (message.kind) {
                    "ButtonPress", "ButtonRelease", "KeyPress", "KeyRelease" -> {
                        packer.packMapHeader(1).packString(message.kind)
                            .packString(message.value ?: "")
                    }
                    "MouseMove", "Wheel" -> {
                        packer.packMapHeader(1).packString(message.kind)
                        packer.packArrayHeader(2)
                            .packDouble(message.x ?: 0.0).packDouble(message.y ?: 0.0)
                    }
                    else -> error("unsupported input kind: ${message.kind}")
                }
            }
            is WireMessage.Shortcut -> {
                packer.packMapHeader(1).packString("Shortcut")
                packer.packArrayHeader(2).packInt(message.modifiers).packString(message.key)
            }
        }
        return packer.toByteArray().also { packer.close() }
    }

    fun decode(bytes: ByteArray): WireMessage {
        val root = MessagePack.newDefaultUnpacker(bytes).use { it.unpackValue() }
        val tagged = tag(root)
        val name = tagged.first
        val payload = tagged.second
        return when (name) {
            "Hello" -> {
                val fields = payload!!.array()
                val screen = fields[1].array()
                val displays = fields[2].array().map {
                    val d = it.array()
                    WireMessage.Display(d[0].int(), d[1].int(), d[2].int(), d[3].int())
                }
                WireMessage.Hello(fields[0].string(), screen[0].int(), screen[1].int(), displays)
            }
            "Enter" -> {
                val f = payload!!.array()
                WireMessage.Enter(f[0].int(), f[1].int(), f[2].string(), f[3].int())
            }
            "EnterAck" -> WireMessage.EnterAck(payload!!.array()[0].asBooleanValue().boolean)
            "Back" -> WireMessage.Back
            "Input" -> decodeInput(payload!!)
            "MoveBy" -> payload!!.array().let { WireMessage.MoveBy(it[0].int(), it[1].int()) }
            "Scroll" -> payload!!.array().let { WireMessage.Scroll(it[0].int(), it[1].int()) }
            "Shortcut" -> payload!!.array().let { WireMessage.Shortcut(it[0].int(), it[1].string()) }
            "Ping" -> WireMessage.Ping(payload!!.long())
            "Pong" -> WireMessage.Pong(payload!!.long())
            "ClipboardRequest" -> WireMessage.ClipboardRequest
            "ClipboardBegin" -> WireMessage.ClipboardBegin(payload!!.array()[0].int())
            "ClipboardChunk" -> WireMessage.ClipboardChunk(payload!!.toByteArray())
            "ClipboardEnd" -> WireMessage.ClipboardEnd
            else -> throw IllegalArgumentException("unsupported wire message: $name")
        }
    }

    /** Decode the desktop's clipboard::Content::Text(String) snapshot. */
    fun decodeClipboardText(bytes: ByteArray): String? {
        val root = MessagePack.newDefaultUnpacker(bytes).use { it.unpackValue() }
        val tagged = tag(root)
        return if (tagged.first == "Text" && tagged.second?.isStringValue == true) {
            tagged.second!!.string()
        } else null
    }

    private fun decodeInput(value: Value): WireMessage.Input {
        val (kind, payload) = tag(value)
        return when (kind) {
            "ButtonPress", "ButtonRelease", "KeyPress", "KeyRelease" ->
                WireMessage.Input(kind, payload!!.string())
            "MouseMove", "Wheel" -> {
                val fields = payload!!.array()
                WireMessage.Input(kind, x = fields[0].double(), y = fields[1].double())
            }
            else -> WireMessage.Input(kind)
        }
    }

    private fun tag(value: Value): Pair<String, Value?> {
        if (value.isStringValue) return value.string() to null
        require(value.isMapValue && value.asMapValue().map().size == 1) { "expected a tagged MessagePack value" }
        val entry = value.asMapValue().map().entries.first()
        return entry.key.string() to entry.value
    }

    private fun Value.array(): List<Value> = this.asArrayValue().list()
    private fun Value.string(): String = this.asStringValue().string()
    private fun Value.int(): Int = this.asIntegerValue().toInt()
    private fun Value.long(): Long = this.asIntegerValue().toLong()
    private fun Value.double(): Double = when {
        isFloatValue -> asFloatValue().toDouble()
        isIntegerValue -> asIntegerValue().toLong().toDouble()
        else -> throw IllegalArgumentException("expected numeric MessagePack value")
    }
    private fun Value.toByteArray(): ByteArray {
        if (isBinaryValue) return asBinaryValue().asByteArray()
        return array().map { it.int().toByte() }.toByteArray()
    }
}
