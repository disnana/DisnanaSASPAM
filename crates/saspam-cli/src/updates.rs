//! GitHub Release discovery for the interactive updater.
//! This module only selects a tag; the existing fail-closed update.sh verifies
//! artifacts, validates configuration, arms its rollback timer and performs the swap.
use std::{
    cmp::Ordering,
    fs,
    io::IsTerminal,
    process::{Command, Stdio},
};
use serde_json::Value;
use crate::{ask_yes, interactive::{choose,t}, root, settings::current_language};

const RELEASES_URL: &str = "https://api.github.com/repos/disnana/DisnanaSASPAM/releases?per_page=25";
const HELPER: &str = "/usr/local/lib/saspam/update.sh";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier { Numeric(u64), Text(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
struct Version { major:u64, minor:u64, patch:u64, pre:Option<Vec<Identifier>> }

impl Ord for Version {
    fn cmp(&self, other:&Self)->Ordering {
        (self.major,self.minor,self.patch).cmp(&(other.major,other.minor,other.patch))
            .then_with(||match (&self.pre,&other.pre) {
                (None,None)=>Ordering::Equal,
                (None,Some(_))=>Ordering::Greater,
                (Some(_),None)=>Ordering::Less,
                (Some(a),Some(b))=>a.cmp(b)
            })
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self,other:&Self)->Option<Ordering>{Some(self.cmp(other))}
}
fn parse_version(tag:&str)->Option<Version> {
    let raw=tag.strip_prefix('v')?;
    let (core,pre)=raw.split_once('-').map_or((raw,None),|(c,p)|(c,Some(p)));
    let mut digits=core.split('.');
    let major=digits.next()?.parse().ok()?;
    let minor=digits.next()?.parse().ok()?;
    let patch=digits.next()?.parse().ok()?;
    if digits.next().is_some(){return None;}
    let pre=pre.map(|s|s.split('.').map(|v|{
        if v.is_empty() || !v.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-'){return None;}
        if v.bytes().all(|b|b.is_ascii_digit()){Some(Identifier::Numeric(v.parse().ok()?))}
        else {Some(Identifier::Text(v.to_owned()))}
    }).collect::<Option<Vec<_>>>()).transpose()?;
    if matches!(pre,Some(ref v) if v.is_empty()){return None;}
    Some(Version{major,minor,patch,pre})
}
#[derive(Debug, Clone)]
struct Release { tag:String, version:Version, prerelease:bool }

fn releases_from_json(data:&str,arch:&str)->Result<Vec<Release>,String> {
    let input:Value=serde_json::from_str(data).map_err(|_|"invalid GitHub Release response")?;
    let entries=input.as_array().ok_or("expected a list of GitHub Releases")?;
    let mut choices=Vec::new();
    for item in entries {
        if item.get("draft").and_then(Value::as_bool)==Some(true){continue;}
        let Some(tag)=item.get("tag_name").and_then(Value::as_str) else {continue;};
        let Some(version)=parse_version(tag) else {continue;};
        let Some(assets)=item.get("assets").and_then(Value::as_array) else {continue;};
        let required=format!("saspam-{tag}-linux-{arch}.tar.gz");
        let mut archive=false;
        let mut checksums=false;
        for asset in assets {
            match asset.get("name").and_then(Value::as_str){
                Some(name) if name==required => archive=true,
                Some("SHA256SUMS")=>checksums=true,
                _=>{},
            }
        }
        if archive && checksums && !choices.iter().any(|r:&Release|r.tag==tag){
            choices.push(Release{tag:tag.to_owned(),version,prerelease:item.get("prerelease").and_then(Value::as_bool).unwrap_or(false)});
        }
    }
    choices.sort_unstable_by(|a,b|b.version.cmp(&a.version));
    choices.truncate(20);
    if choices.is_empty(){return Err("No compatible published GitHub Release found (or all releases are drafts)".into());}
    Ok(choices)
}

fn discover_releases()->Result<Vec<Release>,String>{
    let arch=match std::env::consts::ARCH {
        "x86_64"=>"amd64",
        "aarch64"=>"arm64",
        _=>return Err("This updater supports only amd64 or arm64".into()),
    };
    let output=Command::new("curl")
        .args(["--proto","=https","--tlsv1.2","--fail","--silent","--show-error",
            "--location","--retry","2","--connect-timeout","5","--max-time","15",
            "--header","Accept: application/vnd.github+json",
            "--header","User-Agent: DisnanaSASPAM", RELEASES_URL])
        .stderr(Stdio::piped()).output().map_err(|e|format!("Cannot query GitHub Releases: {e}"))?;
    if !output.status.success(){return Err("Could not retrieve the Release list from GitHub (network error or API rate limit)".into());}
    if output.stdout.len()>1024*1024 {return Err("GitHub Release response is unexpectedly large".into());}
    let json=std::str::from_utf8(&output.stdout).map_err(|_|"Invalid UTF-8 in GitHub Releases")?;
    releases_from_json(json,arch)
}
fn installed_tag()->String{
    fs::read_to_string("/var/lib/saspam/installed-version")
        .unwrap_or_default().trim().to_owned()
}
fn relation(target:&Version,current:Option<&Version>,language:&str)->String {
    match current {
        None=>t(language,"未確認","unknown").to_owned(),
        Some(x) if target>x=>t(language,"更新可能","newer").to_owned(),
        Some(x) if target<x=>t(language,"古い版","older").to_owned(),
        _=>t(language,"現在の版","installed").to_owned(),
    }
}

pub fn run(args:&[String])->Result<(),String>{
    root()?;
    if args.iter().any(|s|s=="--rollback" || s=="--version") {
        return invoke_helper(args);
    }
    // All other existing flags are forwarded after a release has been selected.
    let auto=args.iter().any(|s|s=="--latest");
    let listing=args.iter().any(|s|s=="--list");
    let known=["--latest","--list","--dry-run","--skip-approval-test","--allow-reinstall","--allow-downgrade"];
    for arg in args { if !known.contains(&arg.as_str()){return Err(format!("Unrecognized updater argument: {arg}"));} }
    if listing && auto {return Err("Use --list or --latest, not both".into());}
    let language=current_language();
    let current=installed_tag();
    let current_version=parse_version(&current);
    let releases=discover_releases()?;
    if !current.is_empty() {
        println!("{}: {current}",t(&language,"インストール済み","Installed"));
    } else {
        println!("{}",t(&language,"現在のバージョンを確認できません","Installed version unknown"));
    }
    let entries:Vec<String>=releases.iter().map(|r|{
        let label=relation(&r.version,current_version.as_ref(),&language);
        let pre=if r.prerelease {t(&language,"試験版","prerelease")}else{t(&language,"安定版","stable")};
        format!("{}  [{} / {}]",r.tag,label,pre)
    }).collect();
    if listing {
        println!("{}",t(&language,"利用可能なGitHub Releases","Available GitHub Releases"));
        for entry in &entries {println!("  {entry}");}
        return Ok(());
    }
    let selected=if auto {
        Some(0)
    } else if std::io::stdin().is_terminal() {
        choose(t(&language,"更新先バージョンを選択","Choose release version"),&entries,0,&language)?
    }else{
        return Err("Noninteractive update requires --latest or --version TAG (use --list to inspect releases)".into());
    };
    let Some(index)=selected else{return Ok(());};
    let chosen=&releases[index];
    let mut forwarded=vec!["--version".to_string(),chosen.tag.clone()];
    for flag in args {
        if flag!="--latest" && flag!="--list" {forwarded.push(flag.clone());}
    }
    if let Some(current)=&current_version {
        match chosen.version.cmp(current) {
            Ordering::Less=>{
                if !forwarded.iter().any(|s|s=="--allow-downgrade"){
                    if !std::io::stdin().is_terminal(){return Err("Explicit --allow-downgrade required".into());}
                    if !ask_yes(t(&language,
                        "この版は現在より古いです。ダウングレードしますか？ (yes/no)",
                        "This release is older. Confirm downgrade? (yes/no)"))?{return Ok(());}
                    forwarded.push("--allow-downgrade".into());
                }
            },
            Ordering::Equal=>{
                if !forwarded.iter().any(|s|s=="--allow-reinstall"){
                    if !std::io::stdin().is_terminal(){
                        println!("{}",t(&language,"すでに最新バージョンです","Already on this version"));
                        return Ok(());
                    }
                    if !ask_yes(t(&language,"同じ版を再インストールしますか？ (yes/no)",
                        "Reinstall the currently installed version? (yes/no)"))?{return Ok(());}
                    forwarded.push("--allow-reinstall".into());
                }
            },
            Ordering::Greater=>{},
        }
    }
    println!("{}: {}",t(&language,"更新先","Target release"),chosen.tag);
    if !auto && !forwarded.iter().any(|s|s=="--dry-run") &&
        !ask_yes(t(&language,
            "復旧用コンソール・既存SSH接続を確認しましたか？ 更新しますか？ (yes/no)",
            "Confirm console recovery and an open SSH session. Proceed? (yes/no)"))? {
        return Ok(());
    }
    invoke_helper(&forwarded)
}

fn invoke_helper(args:&[String])->Result<(),String>{
    let outcome=Command::new("bash").arg(HELPER).args(args)
        .status().map_err(|e|format!("Cannot start updater: {e}"))?;
    if outcome.success(){Ok(())}else{Err(format!("Update failed: {outcome}"))}
}

#[cfg(test)]
mod tests{
    use super::*;
    fn asset(tag:&str)->Value {
        serde_json::json!({
            "tag_name":tag, "draft":false, "prerelease":tag.contains('-'),
            "assets":[{"name":format!("saspam-{tag}-linux-amd64.tar.gz")},{"name":"SHA256SUMS"}]
        })
    }
    #[test]
    fn versions_order_alpha_numerically_and_stable_higher_than_prerelease(){
        assert!(parse_version("v0.1.0").unwrap()>parse_version("v0.1.0-alpha.99").unwrap());
        assert!(parse_version("v0.1.0-alpha.10").unwrap()>parse_version("v0.1.0-alpha.9").unwrap());
        assert!(parse_version("v1.0.0").unwrap()>parse_version("v0.9.99").unwrap());
        assert!(parse_version("version-evil").is_none());
    }
    #[test]
    fn only_published_compatible_release_assets_are_listed(){
        let json=serde_json::to_string(&vec![
            asset("v0.1.0-alpha.4"),asset("v0.1.0-alpha.10"),
            serde_json::json!({"tag_name":"v2.0.0","draft":true,"assets":[{"name":"saspam-v2.0.0-linux-amd64.tar.gz"},{"name":"SHA256SUMS"}]}),
            serde_json::json!({"tag_name":"v1.0.0","draft":false,"assets":[]}),
        ]).unwrap();
        let candidates=releases_from_json(&json,"amd64").unwrap();
        assert_eq!(candidates.len(),2);
        assert_eq!(candidates[0].tag,"v0.1.0-alpha.10");
        assert_eq!(candidates[1].tag,"v0.1.0-alpha.4");
    }
    #[test]
    fn no_usable_release_is_a_hard_error(){
        assert!(releases_from_json("[]","amd64").is_err());
    }
}
