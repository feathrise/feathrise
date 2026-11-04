//! 区分SDK原GPU字节、解码RGBA像素和PNG文件三种摘要，防止编码差异被误当作陈旧截图。
use bevy::{asset::RenderAssetUsages,image::{CompressedImageFormats,ImageSampler,ImageType},prelude::Image};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};

fn hash(bytes:&[u8])->String{Sha256::digest(bytes).iter().map(|byte|format!("{byte:02x}")).collect()}

/// SDK没有在既有DTO中传GPU格式；独立核RGBA证据，并注明原摘要对应RGBA还是BGRA布局。
/// 返回None只表示图与回执跨过了原SDK异步更新，调用者必须等下一次匹配，不能当通过。
pub(crate) fn verify(bytes:&[u8],capture:&Value)->Result<Option<Value>,String>{
    if bytes.len()<24||bytes.len()>128*1024*1024||&bytes[..8]!=b"\x89PNG\r\n\x1a\n"{return Err("冻结游戏PNG签名或字节预算无效".into());}
    let width=u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height=u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width==0||height==0||u64::from(width)*u64::from(height)>16*1024*1024{return Err("冻结游戏PNG超过1600万像素预算".into());}
    let image=Image::from_buffer(bytes,ImageType::Extension("png"),CompressedImageFormats::NONE,true,
        ImageSampler::nearest(),RenderAssetUsages::default()).map_err(|error|error.to_string())?
        .try_into_dynamic().map_err(|error|error.to_string())?.to_rgba8();
    let rgba=hash(image.as_raw());
    if capture["capture"]["width"]!=image.width()||capture["capture"]["height"]!=image.height()
        ||capture["author_tools"]["rgba_sha256"]!=rgba{return Ok(None);}
    let mut bgra=image.into_raw();for pixel in bgra.chunks_exact_mut(4){pixel.swap(0,2);}
    let bgra_hash=hash(&bgra);
    let raw_layout=if capture["capture"]["sha256"]==rgba{"rgba8"}else if capture["capture"]["sha256"]==bgra_hash{"bgra8"}else{return Ok(None);};
    Ok(Some(json!({"png_file_sha256":hash(bytes),"decoded_rgba_sha256":rgba,
        "gpu_raw_sha256":capture["capture"]["sha256"],"matching_gpu_raw_layout":raw_layout,"physical":[width,height]})))
}

#[cfg(test)]
mod tests{
    use super::*;
    // 两像素分别红蓝不等，PNG压缩字节、RGBA和BGRA三者摘要必须各自不同。
    const PNG:&[u8]=&[137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,2,0,0,0,1,8,6,0,0,0,244,34,127,138,0,0,0,17,73,68,65,84,120,156,99,96,151,63,254,255,190,38,119,3,0,15,76,3,128,243,181,16,117,0,0,0,0,73,69,78,68,174,66,96,130];
    fn capture()->Value{json!({"capture":{"width":2,"height":1,"sha256":hash(&[199,31,7,255,11,41,223,128])},
        "author_tools":{"rgba_sha256":hash(&[7,31,199,255,223,41,11,128])}})}
    #[test]
    fn sdk_bgra_raw_hash_is_not_a_png_file_hash(){
        let capture=capture();let proof=verify(PNG,&capture).unwrap().unwrap();
        assert_eq!(proof["matching_gpu_raw_layout"],"bgra8");
        assert_ne!(proof["png_file_sha256"],proof["gpu_raw_sha256"]);
        assert_ne!(proof["decoded_rgba_sha256"],proof["gpu_raw_sha256"]);
        assert_eq!(proof["png_file_sha256"],hash(PNG));
    }
    #[test]
    fn stale_pixels_or_relabelled_png_hash_cannot_match_sdk_receipt(){
        let mut changed=capture();changed["capture"]["sha256"]=json!(hash(PNG));
        assert!(verify(PNG,&changed).unwrap().is_none());
        changed=capture();changed["author_tools"]["rgba_sha256"]=json!("0".repeat(64));
        assert!(verify(PNG,&changed).unwrap().is_none());
    }
}
