use overtalk_cloud_gateway::{config::Config, ledger::Ledger, server::App};
use std::{convert::Infallible, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    if let Some(parent) = config
        .database
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&config.database)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            file.metadata()?.permissions().mode() & 0o077 == 0,
            "Credit database must be private"
        );
    }
    drop(file);
    let db = Ledger::open(&config.database).await?;
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "reconcile") {
        anyhow::ensure!(
            args.len() == 4,
            "Usage: overtalk-cloud-gateway reconcile REQUEST_ID VERIFIED_MICRO_USD"
        );
        db.settle(&args[2], args[3].parse()?).await?;
        println!("Reservation reconciled");
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "entitle") {
        anyhow::ensure!(
            args.len() == 6,
            "Usage: overtalk-cloud-gateway entitle ACCOUNT_ID INVOICE_REF PAID_CENTS UNTIL_YYYY-MM-DD (one seat of an invoiced team plan)"
        );
        let until = overtalk_cloud_gateway::date(&args[5])?;
        anyhow::ensure!(
            until > overtalk_cloud_gateway::now(),
            "The period must end in the future"
        );
        db.entitle(
            &args[2],
            &args[3],
            args[4].parse()?,
            overtalk_cloud_gateway::now(),
            until,
        )
        .await?;
        println!("Seat entitled until {}", args[5]);
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "keys") {
        let now = overtalk_cloud_gateway::now();
        match args.get(2).map(String::as_str) {
            // One key per line: LABEL<TAB>KEY<TAB>EXPIRES_YYYY-MM-DD, from a private file.
            Some("import") if args.len() == 4 => {
                let text =
                    overtalk_cloud_gateway::config::private_file(std::path::Path::new(&args[3]))?;
                let (mut added, mut skipped) = (0, 0);
                for (number, line) in text
                    .lines()
                    .enumerate()
                    .filter(|(_, l)| !l.trim().is_empty())
                {
                    let fields: Vec<_> = line.split('\t').collect();
                    anyhow::ensure!(
                        fields.len() == 3,
                        "Line {}: expected LABEL, KEY and EXPIRES separated by tabs",
                        number + 1
                    );
                    let expires = overtalk_cloud_gateway::date(fields[2].trim())?;
                    if db
                        .import_key(fields[0].trim(), fields[1].trim(), expires, now)
                        .await?
                    {
                        added += 1;
                    } else {
                        skipped += 1;
                    }
                }
                println!(
                    "Imported {added} keys ({skipped} already known); {} free",
                    db.free_keys(now).await?
                );
            }
            Some("stale") if args.len() == 3 => {
                for key in db.stale_keys(now).await? {
                    println!("{}\t{}\t{}", key.id, key.label, key.account);
                }
                println!("Delete these keys in the Privatemode portal, then run `keys retire ID` for each.");
            }
            Some("retire") if args.len() == 4 => {
                db.retire_key(&args[3], now).await?;
                println!("Key retired");
            }
            _ => anyhow::bail!(
                "Usage: overtalk-cloud-gateway keys import FILE | keys stale | keys retire ID"
            ),
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "recover") {
        anyhow::ensure!(args.len()==3,"Usage: overtalk-cloud-gateway recover ACCOUNT_ID (verify the customer's purchase first)");
        let token = overtalk_cloud_gateway::secret()?;
        db.rotate(&args[2], &token).await?;
        println!("{token}");
        return Ok(());
    }
    let bind = config.bind;
    let app = Arc::new(App::new(config, db)?);
    if app.config.approved {
        app.stripe.validate_prices().await?;
    }
    tokio::spawn(app.clone().billing_worker());
    let make =
        hyper::service::make_service_fn(move |connection: &hyper::server::conn::AddrStream| {
            let app = app.clone();
            let ip = connection.remote_addr().ip();
            async move {
                Ok::<_, Infallible>(hyper::service::service_fn(move |request| {
                    let app = app.clone();
                    async move { Ok::<_, Infallible>(app.handle(request, ip).await) }
                }))
            }
        });
    println!("Cloud gateway listening on {bind}");
    hyper::Server::bind(&bind)
        .http1_header_read_timeout(std::time::Duration::from_secs(10))
        .serve(make)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
