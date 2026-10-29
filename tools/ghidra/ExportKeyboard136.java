// Offline decompilation of the support image, never USB access.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportKeyboard136 extends GhidraScript {
    @Override public void run() throws Exception {
        File out=new File(getScriptArgs()[0]);out.mkdirs();
        DecompInterface d=new DecompInterface();d.openProgram(currentProgram);
        int count=0;long deadline=System.nanoTime()+240_000_000_000L;
        try(PrintWriter code=new PrintWriter(new File(out,"keyboard-136.c"),StandardCharsets.UTF_8);
            PrintWriter index=new PrintWriter(new File(out,"keyboard-136-functions.tsv"),StandardCharsets.UTF_8)) {
            index.println("entry\tname\tbytes\tdecompiled");
            FunctionIterator it=currentProgram.getFunctionManager().getFunctions(true);
            while(it.hasNext() && !monitor.isCancelled() && System.nanoTime()<deadline) {
                Function f=it.next();if(f.getEntryPoint().getOffset()>=0x7e000)continue;
                DecompileResults r=d.decompileFunction(f,5,monitor);
                boolean ok=r.decompileCompleted()&&r.getDecompiledFunction()!=null;
                index.println(f.getEntryPoint()+"\t"+f.getName()+"\t"+f.getBody().getNumAddresses()+"\t"+ok);
                if(ok){code.println("\n/* ENTRY "+f.getEntryPoint()+" */");code.println(r.getDecompiledFunction().getC());count++;}
            }
        }finally{d.dispose();}
        println("Exported "+count+" keyboard functions");
    }
}
