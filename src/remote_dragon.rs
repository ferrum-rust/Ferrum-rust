//! Client dragon multipart positions. The 64-entry flight history is sampled before interpolation.
use crate::vanilla::math::{sin,cos};
use glam::DVec3;
pub(crate) const SIZES:[[f32;2];7]=[[3.,3.],[5.,3.],[2.,2.],[2.,2.],[2.,2.],[4.,2.],[4.,3.]];
#[derive(Clone, Debug)]
pub(crate) struct Flight{history:[[f64;2];64],at:usize}
impl Flight{
    pub fn new(y:f64,yaw:f32)->Self{Self{history:[[f64::from(yaw),y];64],at:63}}
    pub fn record(&mut self,y:f64,yaw:f32){self.at=(self.at+1)%64;self.history[self.at]=[f64::from(yaw),y];}
    pub fn offset(&self,n:usize)->[f64;2]{self.history[(self.at+64-n)%64]}
    pub fn movement_offsets(&self, index: usize, partial_ticks: f32, dead: bool) -> [f64; 2] {
        let partial = if dead { 0.0 } else { partial_ticks };
        let t = f64::from(1.0 - partial);
        let i = self.at.wrapping_add(64).wrapping_sub(index) & 63;
        let j = self.at.wrapping_add(64).wrapping_sub(index).wrapping_sub(1) & 63;
        let yaw0 = self.history[i][0];
        let mut dyaw = (self.history[j][0] - yaw0) % 360.0;
        if dyaw >= 180.0 {
            dyaw -= 360.0;
        }
        if dyaw < -180.0 {
            dyaw += 360.0;
        }
        let y0 = self.history[i][1];
        let dy = self.history[j][1] - y0;
        [yaw0 + dyaw * t, y0 + dy * t]
    }
    pub fn parts(&self,p:DVec3,yaw:f32)->[DVec3;7]{
        let speed=self.offset(5);let pitch=(speed[1]-self.offset(10)[1]) as f32*10./180.*std::f32::consts::PI;
        let slope=cos(pitch);let rise=-sin(pitch);let turn=yaw*std::f32::consts::PI/180.;let x=sin(turn);let z=cos(turn);
        let offset=|a:f32,b:f64,c:f32|p+DVec3::new(f64::from(a),b,f64::from(c));
        let mut result=[p;7];
        result[0]=offset(x*5.5*slope,self.offset(0)[1]-speed[1]+f64::from(rise*5.5),-z*5.5*slope);
        result[1]=offset(x*0.5,0.,-z*0.5);
        for i in 0..3{
            let past=self.offset(12+i*2);let delta=((past[0]-speed[0]+180.).rem_euclid(360.)-180.) as f32;
            let rotation=turn+delta*std::f32::consts::PI/180.;let step=(i+1) as f32*2.;
            result[2+i]=offset(-(x*1.5+sin(rotation)*step)*slope,past[1]-speed[1]-f64::from((step+1.5)*rise)+1.5,(z*1.5+cos(rotation)*step)*slope);
        }
        result[5]=offset(z*4.5,2.,x*4.5);result[6]=offset(-z*4.5,2.,-x*4.5);result
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn original_dragon_part_positions_sizes_and_protocol_ids(){
        type Row=([f64;2],[f64;4],Vec<[f64;6]>);
        let rows:Vec<Row>=serde_json::from_reader(flate2::read::GzDecoder::new(&include_bytes!("../tests/fixtures/vanilla189-dragon-parts.json.gz")[..])).unwrap();
        let mut flight=Flight::new(rows[0].0[0],rows[0].0[1] as f32);
        for (i,(before,p,parts)) in rows.into_iter().enumerate(){
            flight.record(before[0],before[1] as f32);
            let actual=flight.parts(DVec3::new(p[0],p[1],p[2]),p[3] as f32);
            for (j,part) in parts.into_iter().enumerate(){
                assert!((actual[j]-DVec3::new(part[0],part[1],part[2])).abs().max_element()<1e-6,"tick={i} part={j} {:?}",actual[j]);
                assert_eq!([part[3] as f32,part[4] as f32],SIZES[j]);assert_eq!(part[5] as usize,j+1);
            }
        }
    }
}
