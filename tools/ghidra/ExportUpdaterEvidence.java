// Export function references around updater protocol source-path strings.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class ExportUpdaterEvidence extends GhidraScript {
    private List<Address> literalUsers(Address target) throws Exception {
        long value = target.getOffset();
        byte[] pointer = new byte[4];
        for (int i = 0; i < 4; i++) pointer[i] = (byte)(value >>> (i * 8));
        List<Address> found = new ArrayList<>();
        Address at = currentProgram.getImageBase();
        while (found.size() < 100 && (at = currentProgram.getMemory().findBytes(at, pointer, null, true, monitor)) != null) {
            found.add(at); at = at.add(1);
        }
        return found;
    }
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Expected output directory");
        File out = new File(args[0]);
        if (!out.isDirectory()) throw new IOException("Missing output directory");
        Set<Function> functions = new TreeSet<>(Comparator.comparing(f -> f.getEntryPoint()));
        DecompInterface decompiler = new DecompInterface();
        if (!decompiler.openProgram(currentProgram)) throw new IOException("Cannot open decompiler");
        try (PrintWriter code = new PrintWriter(new File(out, "dock-updater-protocol-evidence.c"), StandardCharsets.UTF_8);
             PrintWriter refs = new PrintWriter(new File(out, "dock-updater-protocol-refs.tsv"), StandardCharsets.UTF_8)) {
            refs.println("source_path\tstring_address\treference\tfunction");
            for (String source : new String[]{"dangle4k.rs", "oled.rs", "flash.rs", "mled.rs"}) {
                byte[] needle = ("src\\upgrade\\method\\" + source).getBytes(StandardCharsets.US_ASCII);
                Address at = currentProgram.getImageBase();
                while ((at = currentProgram.getMemory().findBytes(at, needle, null, true, monitor)) != null) {
                    refs.println(source + "\t" + at + "\tfound\tstring");
                    for (Reference ref : getReferencesTo(at)) {
                        Function f = getFunctionContaining(ref.getFromAddress());
                        refs.println(source + "\t" + at + "\t" + ref.getFromAddress() + "\t" + (f == null ? "data" : f.getEntryPoint()));
                        if (f != null) functions.add(f);
                        else for (Reference indirect : getReferencesTo(ref.getFromAddress())) {
                            f = getFunctionContaining(indirect.getFromAddress());
                            refs.println(source + "\t" + at + "\t" + indirect.getFromAddress() + "\t" + (f == null ? "data" : f.getEntryPoint()));
                            if (f != null) functions.add(f);
                        }
                    }
                    // Bounded byte-level fallback: a timed-out Rust analysis may
                    // not have created references for panic-location structures.
                    for (Address cell : literalUsers(at)) {
                        for (Address user : literalUsers(cell)) {
                            Function f = getFunctionContaining(user);
                            refs.println(source + "\t" + cell + "\t" + user + "\t" + (f == null ? "unresolved_literal" : f.getEntryPoint()));
                            if (f != null) functions.add(f);
                        }
                    }
                    at = at.add(needle.length);
                }
            }
            int count = 0;
            for (Function f : functions) {
                if (++count > 70 || monitor.isCancelled()) break;
                DecompileResults result = decompiler.decompileFunction(f, 8, monitor);
                if (result.decompileCompleted() && result.getDecompiledFunction() != null) {
                    code.println("\n/* ENTRY " + f.getEntryPoint() + " */");
                    code.println(result.getDecompiledFunction().getC());
                }
            }
            println("Updater protocol reference functions: " + functions.size());
        } finally { decompiler.dispose(); }
    }
}
