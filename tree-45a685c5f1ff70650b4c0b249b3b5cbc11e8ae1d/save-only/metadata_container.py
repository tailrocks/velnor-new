"""Recognize supported AppleDouble metadata containers without inspecting secrets."""
import struct
def is_appledouble(prefix, size):
    if len(prefix) < 26:
        return False
    magic, version = struct.unpack_from(">II", prefix)
    count = struct.unpack_from(">H", prefix, 24)[0]
    table_end = 26 + 12 * count
    if magic != 0x00051607 or version not in (0x00010000, 0x00020000) or count == 0:
        return False
    if table_end > len(prefix) or table_end > size:
        return False
    records = [struct.unpack_from(">III", prefix, 26 + 12 * index) for index in range(count)]
    return all(identifier > 0 and offset >= table_end and offset + length <= size
               for identifier, offset, length in records)
