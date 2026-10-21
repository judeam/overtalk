"""Bounded Cohere transcription and audited MOSS joint speech/speaker inference."""
from __future__ import annotations
import re
import math
from .asr import SpeechError, cached_checkpoint, SAMPLE_RATE
COHERE_LANGUAGES={"en","de","fr","es","it","pt","nl","pl","el","ar","ja","zh","ko","vi"}
MODERN_MODELS={
 "cohere-transcribe":("CohereLabs/cohere-transcribe-03-2026","SONEFOLD_COHERE_MODEL_DIR"),
 "moss-transcribe-diarize":("OpenMOSS-Team/MOSS-Transcribe-Diarize","SONEFOLD_MOSS_MODEL_DIR"),
}

def load_model(name,device,dtype):
    checkpoint=cached_checkpoint(*MODERN_MODELS[name])
    if name=="cohere-transcribe":
        from transformers import AutoProcessor,CohereAsrForConditionalGeneration
        processor=AutoProcessor.from_pretrained(str(checkpoint),local_files_only=True,trust_remote_code=False)
        model=CohereAsrForConditionalGeneration.from_pretrained(str(checkpoint),local_files_only=True,
            trust_remote_code=False,use_safetensors=True,torch_dtype=dtype,attn_implementation="sdpa").to(device).eval()
    else:
        from .vendor.moss import MossTranscribeDiarizeForConditionalGeneration,MossTranscribeDiarizeProcessor
        from transformers import AutoFeatureExtractor,AutoTokenizer
        import json
        # Construct reviewed classes explicitly. Never execute cached repository code.
        feature=AutoFeatureExtractor.from_pretrained(str(checkpoint),local_files_only=True,trust_remote_code=False)
        tokenizer=AutoTokenizer.from_pretrained(str(checkpoint),local_files_only=True,trust_remote_code=False)
        config=json.loads((checkpoint/"processor_config.json").read_text()) if (checkpoint/"processor_config.json").is_file() else {}
        allowed={k:v for k,v in config.items() if k in {"audio_tokens_per_second","audio_merge_size","time_marker_every_seconds","enable_time_marker"}}
        template=(checkpoint/"chat_template.jinja").read_text() if (checkpoint/"chat_template.jinja").is_file() else tokenizer.chat_template
        processor=MossTranscribeDiarizeProcessor(feature_extractor=feature,tokenizer=tokenizer,chat_template=template,**allowed)
        model=MossTranscribeDiarizeForConditionalGeneration.from_pretrained(str(checkpoint),local_files_only=True,
            trust_remote_code=False,use_safetensors=True,torch_dtype=dtype,attn_implementation="sdpa").to(device).eval()
    return model,processor

def validate_moss(raw,duration,offset=0):
    # Upstream parser can resynchronize after malformed text. Reject dropped output.
    pattern=re.compile(r"\[([0-9]+(?:\.[0-9]+)?)\]\[(S[0-9]+)\](.*?)\[([0-9]+(?:\.[0-9]+)?)\]",re.S)
    matches=list(pattern.finditer(raw));rest=pattern.sub("",raw).strip()
    if rest or (raw.strip() and not matches):
        raise SpeechError("MOSS returned incomplete speaker/timestamp output; original transcript is unchanged")
    segments=[]
    for match in matches:
        start,end=float(match[1]),float(match[4]);text=match[3].strip()
        if not 0<=start<end<=duration+.1:
            raise SpeechError("MOSS returned timestamps outside the recording")
        if text:segments.append({"text":text,"start":round(offset+start,3),"end":round(offset+min(end,duration),3),"speaker":match[2],"timing":"model-segment"})
    return segments

def transcribe_window(loaded,samples,language,name,offset,duration,torch):
    model,processor=loaded
    if name=="cohere-transcribe":
        if language not in COHERE_LANGUAGES:
            raise SpeechError("Cohere needs a supported language selected in Settings; use Qwen for automatic language detection")
        inputs=processor(samples,sampling_rate=SAMPLE_RATE,return_tensors="pt",language=language,punctuation=True)
        chunk_index=inputs.pop("audio_chunk_index",None)
        tensors={k:v.to(model.device, dtype=model.dtype) if k=="input_features" else v.to(model.device) for k,v in inputs.items()}
        with torch.inference_mode():tokens=model.generate(**tensors,max_new_tokens=1024)
        if tokens.shape[-1]>=1024:
            raise SpeechError("Cohere reached the output limit; refusing a possibly truncated transcript")
        decoded=processor.decode(tokens,skip_special_tokens=True,audio_chunk_index=chunk_index,language=language)
        text=" ".join(decoded).strip() if isinstance(decoded,list) else str(decoded).strip()
        segments=[{"text":text,"start":round(offset,3),"end":round(offset+duration,3),"speaker":None,"timing":"chunk"}] if text else []
        return text,segments,["Cohere timestamps identify bounded audio chunks; no word alignment is claimed"]
    from .vendor.moss.inference_utils import generate_transcription,DEFAULT_PROMPT
    # In-memory PCM prevents the helper from opening URLs or untrusted files.
    messages=[{"role":"user","content":[{"type":"audio","audio":samples},{"type":"text","text":DEFAULT_PROMPT}]}]
    budget=min(16384,max(2048,math.ceil(duration*16)))
    output=generate_transcription(model,processor,messages,max_length=40000,max_new_tokens=budget,
                                    do_sample=False,device=model.device,dtype=model.dtype)
    if output["generated_tokens"]>=budget:raise SpeechError("MOSS reached the output limit; refusing a partial transcript")
    segments=validate_moss(output["text"],duration,offset)
    return " ".join(s["text"] for s in segments),segments,[]

def transcribe_joint(engine,path):
    import numpy as np
    from .asr import audio_windows
    chunks=[];duration=0.0
    for samples,offset,length in audio_windows(path):
        duration=offset+length
        if duration>1800:
            raise SpeechError("MOSS joint analysis is limited to 30 minutes to bound desktop memory; use Qwen + whole-meeting Nemotron diarization for longer recordings")
        chunks.append(samples)
    if not chunks:return {"text":"","segments":[],"speakers":[],"turns":[],"exclusive_turns":[],"warnings":[],"audio_seconds":0.0,"model":"moss-transcribe-diarize","language":None}
    loaded=engine._load("moss-transcribe-diarize")
    text,segments,warnings=transcribe_window(loaded,np.concatenate(chunks),None,"moss-transcribe-diarize",0,duration,engine._torch)
    turns=[{"start":s["start"],"end":s["end"],"speaker":s["speaker"]} for s in segments]
    speakers=sorted({s["speaker"] for s in segments})
    return {"text":text,"segments":segments,"speakers":[{"id":s,"name":s} for s in speakers],"turns":turns,"exclusive_turns":[],"warnings":warnings,"audio_seconds":duration,"model":"moss-transcribe-diarize","diarization_model":"moss-joint","language":None}
