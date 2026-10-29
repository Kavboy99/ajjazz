// Seed the previously excluded signed controller API table; no USB access.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

public class SeedPanControllerApi extends GhidraScript {
    @Override public void run() throws Exception {
        int seeded = 0;
        // The pinned receiver's first scatter entry is a direct word copy.
        // Its ROM thunk at10A9A was inspected separately. Do not generalise
        // the other scatter entries: one uses decompression, another zeroing.
        if (!currentProgram.getName().equals("dock-MV303-DV233-reference.bin")) {
            throw new IllegalArgumentException("RAM relocation audited only for pinned receiver reference");
        }
        long table = Integer.toUnsignedLong(getInt(toAddr(0x10474)));
        if (table != 0x27DC4L || getInt(toAddr(table)) != 0x27DF4 ||
            getInt(toAddr(table + 4)) != 0x2000C800 || getInt(toAddr(table + 8)) != 0x2B28 ||
            getInt(toAddr(table + 12)) != 0x10A9A) {
            throw new IllegalArgumentException("Pinned controller scatter entry differs");
        }
        Memory memory = currentProgram.getMemory();
        Address destination = toAddr(0x2000C800L), end = destination.add(0x2B28);
        MemoryBlock ram = memory.getBlock(destination);
        if (!ram.getStart().equals(destination)) memory.split(ram, destination);
        ram = memory.getBlock(destination);
        if (ram.getEnd().compareTo(end) >= 0) memory.split(ram, end);
        ram = memory.getBlock(destination);
        if (!ram.isInitialized()) ram = memory.convertToInitialized(ram, (byte)0);
        ram.setExecute(true);
        memory.setBytes(destination, getBytes(toAddr(0x27DF4), 0x2B28));
        // Jump trampolines point into the copied RAM code. Seed their targets.
        for (long at = 0x1024C; at < 0x103C0; at += 12) {
            long targetValue = Integer.toUnsignedLong(getInt(toAddr(at + 8))) & ~1L;
            if (targetValue < 0x2000C800L || targetValue >= 0x2000F328L) continue;
            Address target = toAddr(targetValue);
            disassemble(target);
            if (getFunctionAt(target) == null) createFunction(target, "controller_ram_" + Long.toHexString(targetValue));
            currentProgram.getSymbolTable().addExternalEntryPoint(target);
            seeded++;
        }
        // At10200 there are Thumb stubs, not a Cortex reset vector. Seventeen
        // pointers end at1024C; code trampolines follow. RAM targets need relocation.
        for (long offset = 0x10208; offset < 0x1024C; offset += 4) {
            long pointer = Integer.toUnsignedLong(getInt(toAddr(offset)));
            long value = pointer & ~1L;
            if ((pointer & 1) == 0 || !((value >= 0x1024C && value < 0x30000) ||
                (value >= 0x2000C800L && value < 0x2000F328L))) continue;
            Address target = toAddr(value);
            // Existing projects already have Thumb context over the image.
            // Rewriting context across existing instructions is rejected by Ghidra.
            disassemble(target);
            if (getFunctionAt(target) == null) createFunction(target, "controller_api_" + Long.toHexString((offset - 0x10208) / 4));
            currentProgram.getSymbolTable().addExternalEntryPoint(target);
            seeded++;
        }
        println("Controller API entries seeded: " + seeded + "; direct-copy RAM code restored for analysis only");
    }
}
