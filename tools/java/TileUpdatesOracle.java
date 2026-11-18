import com.google.gson.Gson;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.*;
import net.minecraft.nbt.*;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.network.NetHandlerPlayClient;
import io.netty.buffer.Unpooled;

/** Original NetHandlerPlayClient S35/S33 dispatch and real tile read/write snapshots. */
public final class TileUpdatesOracle {
    static int[] ids={52,137,138,144,140,176};
    static TileEntity tile(int type){return new TileEntity[]{new TileEntityMobSpawner(),new TileEntityCommandBlock(),new TileEntityBeacon(),
        new TileEntitySkull(),new TileEntityFlowerPot(),new TileEntityBanner()}[type-1];}
    static NBTTagCompound state(TileEntity te){NBTTagCompound n=new NBTTagCompound();te.writeToNBT(n);return n;}
    static NBTTagCompound payload(int type,int n,BlockPos pos,TileEntity te) {
        NBTTagCompound p=n%5==0?new NBTTagCompound():state(te);
        p.setInteger("x",pos.getX());p.setInteger("y",pos.getY());p.setInteger("z",pos.getZ());
        p.setString("extraUnrecognizedKey","discard");
        switch(type){
        case 1:
            p.setString("EntityId",n%4==1?"Minecart":n%4==2?"Zombie":n%4==3?"":"Pig");
            p.setInteger("Delay",n*997);
            if(n%2==0){p.setDouble("MinSpawnDelay",31.3);p.setInteger("SpawnCount",12);}
            if(n%3!=0){NBTTagCompound spawn=new NBTTagCompound();spawn.setByte("Type",(byte)(n%7));spawn.setString("CustomName","test");
                p.setTag("SpawnData",spawn);}
            if(n%4==1){NBTTagList potentials=new NBTTagList();
                NBTTagCompound e=new NBTTagCompound();e.setString("Type","Minecart");e.setInteger("Weight",n);
                NBTTagCompound a=new NBTTagCompound();a.setInteger("Type",n%7);e.setTag("Properties",a);potentials.appendTag(e);p.setTag("SpawnPotentials",potentials);} break;
        case 2:p.setString("Command","say slot"+n);p.setInteger("SuccessCount",n-5);
            if(n%2==0)p.setString("CustomName","Pick Block");
            p.setBoolean("TrackOutput",n%3!=0);
            if(n%4==0)p.setString("LastOutput","{\"text\":\"Done\"}");
            if(n%2==1){NBTTagCompound stats=new NBTTagCompound();stats.setString("SuccessCountName","name");stats.setString("SuccessCountObjective","objective");p.setTag("CommandStats",stats);}break;
        case 3:p.setInteger("Primary",new int[]{1,3,5,8,10,11,2,99,-1}[n%9]);p.setInteger("Secondary",n%16);p.setShort("Levels",(short)(n-2));p.setString("Lock","beacon");break;
        case 4:p.setInteger("SkullType",n%6);p.setInteger("Rot",255-n);
            if(n%6==3){NBTTagCompound owner=new NBTTagCompound();owner.setString("Name","Owner"+n);owner.setString("Id",new UUID(n,189).toString());owner.setString("Unknown","discard");p.setTag("Owner",owner);}break;
        case 5:if(n%2==0)p.setInteger("Item",n%3==0?0:6);else p.setString("Item",n%3==0?"unknown:item":"minecraft:red_flower");p.setInteger("Data",n%16);break;
        case 6:p.setInteger("Base",n-4);if(n%2==0){NBTTagList patterns=new NBTTagList();NBTTagCompound pat=new NBTTagCompound();pat.setInteger("Color",n%16);pat.setString("Pattern","bo");patterns.appendTag(pat);p.setTag("Patterns",patterns);}break;
        }
        return p;
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        MiddlePickOracle h=new MiddlePickOracle();
        EntityPickOracle.field(h.mc,Minecraft.class,"mcThread",Thread.currentThread());
        EntityPickOracle.field(h.net,NetHandlerPlayClient.class,"clientWorldController",h.w);
        BlockPos pos=h.pos;h.w.getChunkFromChunkCoords(pos.getX()>>4,pos.getZ()>>4);
        List<Object> rows=new ArrayList<Object>(),signs=new ArrayList<Object>();
        for(int type=1;type<=6;type++) {
            TileEntity t=tile(type);t.setWorldObj(h.w);t.setPos(pos);h.w.tiles.put(pos,t);int id=ids[type-1];
            h.w.blocks.put(pos,(id<<4)|(id==144?1:0));
            for(int n=0;n<31;n++){
                int code=n%11==0?(type%6)+1:type;
                NBTTagCompound before=state(t),data=payload(type,n,pos,t);
                S35PacketUpdateTileEntity packet=new S35PacketUpdateTileEntity(pos,code,data);
                PacketBuffer wire=new PacketBuffer(Unpooled.buffer());packet.writePacketData(wire);
                S35PacketUpdateTileEntity copy=new S35PacketUpdateTileEntity();copy.readPacketData(wire);
                h.net.handleUpdateTileEntity(copy);
                rows.add(Arrays.asList(id,MiddlePickOracle.tag(before),MiddlePickOracle.hex(wire),MiddlePickOracle.tag(state(t))));
            }
        }
        String[] content={"\"\"","\"hello <&> 한글\"","{\"text\":\"Hi\",\"bold\":true,\"color\":\"gold\"}",
            "[\"first\",{\"text\":\" next\",\"italic\":true}]","{\"translate\":\"chat.type.text\",\"with\":[\"A\",\"B\"]}",
            "{\"text\":\"shown\",\"hoverEvent\":{\"action\":\"show_text\",\"value\":\"tip\"}}",
            "{\"selector\":\"@p\"}","{\"text\":\"url\",\"clickEvent\":{\"action\":\"open_url\",\"value\":\"https://example.invalid\"}}",
            "{\"score\":{\"name\":\"n\",\"objective\":\"test\",\"value\":\"17\"}}"};
        for(int n=0;n<content.length;n++)for(boolean editable:new boolean[]{true,false}){
            TileEntitySign t=new TileEntitySign();t.setWorldObj(h.w);t.setPos(pos);t.setEditable(editable);h.w.tiles.put(pos,t);h.w.blocks.put(pos,63<<4);
            IChatComponent[] lines=new IChatComponent[4];for(int i=0;i<4;i++)lines[i]=IChatComponent.Serializer.jsonToComponent(content[(n+i)%content.length]);
            S33PacketUpdateSign packet=new S33PacketUpdateSign(h.w,pos,lines);
            PacketBuffer wire=new PacketBuffer(Unpooled.buffer());packet.writePacketData(wire);S33PacketUpdateSign copy=new S33PacketUpdateSign();copy.readPacketData(wire);
            NBTTagCompound before=state(t);h.net.handleUpdateSign(copy);
            signs.add(Arrays.asList(editable,MiddlePickOracle.tag(before),MiddlePickOracle.hex(wire),MiddlePickOracle.tag(state(t))));
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("tile",rows);result.put("sign",signs);
        List<Object> chat=new ArrayList<Object>();
        for(String plain:new String[]{"0","true","false","\"one <&>= ' two\"","{\"text\":9}","{\"text\":\"hi\",\"unknown\":1}",
              "[\"one\",\" two\",\" three\"]","[[\"x\",\"y\"],{\"translate\":\"key\"}]",
              "{\"translate\":\"chat\",\"with\":[2,{\"text\":\"str\"},{\"selector\":\"@p\"}]}",
              "{\"score\":{\"name\":\"name\",\"objective\":\"o\",\"value\":12}}","{\"selector\":\"@e\"}"}) {
            chat.add(Arrays.asList(plain,IChatComponent.Serializer.componentToJson(IChatComponent.Serializer.jsonToComponent(plain))));
            for(String style:new String[]{"\"bold\":false","\"bold\":\"True\",\"italic\":true","\"underlined\":true,\"obfuscated\":\"no\"","\"color\":\"dark_red\"","\"color\":\"UNKNOWN\"",
                  "\"strikethrough\":false,\"insertion\":\"q<&>\"","\"clickEvent\":{\"action\":\"open_file\",\"value\":\"local\"}",
                  "\"clickEvent\":{\"action\":\"suggest_command\",\"value\":\"/say <hello>\"}",
                  "\"hoverEvent\":{\"action\":\"show_text\",\"value\":[\"tip\",{\"text\":\"!\",\"bold\":true}]}"}) {
                String s="{\"extra\":["+plain+"],\"text\":\"ROOT\","+style+"}";
                chat.add(Arrays.asList(s,IChatComponent.Serializer.componentToJson(IChatComponent.Serializer.jsonToComponent(s))));
            }
        }
        result.put("chat",chat);
        out.println(new Gson().toJson(result));
    }
}
