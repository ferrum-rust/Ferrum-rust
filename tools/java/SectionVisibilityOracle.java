import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.client.renderer.chunk.*;
import net.minecraft.util.*;

public class SectionVisibilityOracle {
    static boolean solid(int mode, int index, Random r) {
        int x=index&15, z=(index>>4)&15, y=index>>8;
        switch (mode) {
            case 0: return false;
            case 1: return true;
            case 2: return x==7;
            case 3: return x==7 || z==7;
            case 4: return !(x==7&&y==7 || z==7&&y==7);
            case 5: return index<255;
            case 6: return index<256;
            default: return r.nextInt(100)<(mode-6)*10;
        }
    }
    public static void main(String[] argv)throws Exception{
        List<Object> rows=new ArrayList<>();
        for(int seed=0;seed<8;seed++)for(int mode=0;mode<17;mode++){
            Random random=new Random(seed);
            VisGraph graph=new VisGraph(); VisGraph camera=new VisGraph();
            byte[] bits=new byte[512];
            for(int i=0;i<4096;i++)if(solid(mode,i,random)){
                BlockPos pos=new BlockPos(i&15,i>>8,i>>4&15);
                graph.func_178606_a(pos);camera.func_178606_a(pos);bits[i/8]|=(byte)(1<<(i%8));
            }
            SetVisibility portals=graph.computeVisibility();long faces=0;
            for(EnumFacing a:EnumFacing.values())for(EnumFacing b:EnumFacing.values())
                if(portals.isVisible(a,b))faces|=1L<<(a.ordinal()*6+b.ordinal());
            int start=random.nextInt(4096);int exits=0;
            for(EnumFacing f:camera.func_178609_b(new BlockPos(start&15,start>>8,start>>4&15)))exits|=1<<f.ordinal();
            rows.add(Arrays.asList(bits,faces,start,exits));
        }
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[0])),"UTF-8")){
            new Gson().toJson(rows,out);
        }
    }
}
