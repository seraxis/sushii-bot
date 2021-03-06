use regex::Regex;
use std::fmt::Write;
use reqwest::Client;
use utils::config::get_pool;
use vlive::ReqwestVLiveRequester;
use utils::numbers::comma_number;
use serenity::framework::standard::CommandError;
use utils::arg_types;

command!(vlive(_ctx, msg, args) {
    let subcommand = match args.single::<String>() {
        Ok(val) => val,
        Err(_) => return Err(CommandError::from(get_msg!("vlive/error/missing_or_invalid_subcommand"))),
    };

    let query = args.rest();
        
    if query.is_empty() {
        return Err(CommandError::from(get_msg!("vlive/error/missing_query")));
    }

    let _ = msg.channel_id.broadcast_typing();

    let client = Client::new();

    match subcommand.as_ref() {
        "search" | "channel" => {
            // channel list, maybe lazy_static this?
            let channels = match client.get_channel_list() {
                Ok(val) => val,
                Err(why) => {
                    warn_discord!("Err searching vlive '{}': {:?}", query, why);

                    return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
                },
            };
            
            // search channel in list
            let channel = match channels.find_partial_channel(query) {
                Some(val) => val,
                None => return Err(CommandError::from(get_msg!("vlive/error/no_search_results"))),
            };

            // get channel code
            let channel_code = match channel.code {
                Some(val) => val,
                None => return Err(CommandError::from(get_msg!("vlive/error/invalid_channel"))),
            };

            // fetch decoded channel code
            let channel_seq = match client.decode_channel_code(channel_code) {
                Ok(val) => val,
                Err(e) => {
                    warn_discord!("Error decoding channel: {}", e);

                    return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
                }
            };

            // get channel videos
            let channel_data = match client.get_channel_video_list(channel_seq as u32, 10, 1) {
                Ok(val) => val,
                Err(e) => {
                    warn_discord!("Error decoding channel: {}", e);

                    return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
                }
            };

            let channel_color = u64::from_str_radix(&channel_data.channel_info.representative_color.replace("#", ""), 16);

            let _ = msg.channel_id.send_message(|m| m
                .embed(|e| {
                    let mut e = e
                        .title(&format!("{}", channel_data.channel_info.channel_name))
                        .url(&channel_data.channel_info.url())
                        .thumbnail(&channel_data.channel_info.channel_profile_image)
                        .footer(|f| f
                            .text(&format!("{} channel fans", comma_number(channel_data.channel_info.fan_count.into())))
                        );
                    
                    if let Ok(color) = channel_color {
                        e = e.colour(color);
                    }

                    if let Some(video) = channel_data.video_list.first() {
                        e = e
                            .image(&video.thumbnail)
                            .field("Latest Video", &format!("**[{}]({})**", video.title, video.url()), false)
                            .field("Plays", &comma_number(video.play_count.into()), true)
                            .field("Hearts", &comma_number(video.like_count.into()), true)
                            .field("Comments", &comma_number(video.comment_count.into()), true)
                            .timestamp(video.on_air_start_at.to_rfc3339());
                    }

                    e
                })
            );
        },
        "video" => {
            lazy_static! {
                static ref RE: Regex = Regex::new(r"vlive\.tv/video/(\d+)").unwrap();
            }

            let video_seq = match RE.captures(&query)
                .and_then(|caps| caps.get(1))
                .map(|cap| cap.as_str())
                .and_then(|num| num.parse::<u32>().ok()) {
                Some(val) => val,
                None => return Err(CommandError::from(get_msg!("vlive/error/invalid_video"))),
            };

            let mut video = match client.get_video(video_seq) {
                Ok(val) => val,
                Err(why) => {
                    warn_discord!("Err searching vlive '{}': {}", query, why);

                    return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_or_not_vod")));
                },
            };

            if video.videos.list.is_empty() {
                return Err(CommandError::from(get_msg!("vlive/error/no_videos")));
            }

            let mut video_links = String::new();
            let mut is_ch_plus = false;

            // sort videos by size
            video.videos.list.sort_by(|a, b| 
                b.size.cmp(&a.size)
            );

            // only use top 3 videos to not go over embed limits
            video.videos.list.truncate(3);

            for vid in &video.videos.list {
                let _ = write!(video_links, "[**{}**]({}) ({} MB) - {}kbps video {}kbps audio\n",
                    vid.encoding_option.name,
                    vid.source,
                    vid.size / 1048576,
                    vid.bitrate.video,
                    vid.bitrate.audio);
                if vid.source.contains("&duration=30") {
                    is_ch_plus = true;
                }
            }

            if video_links.is_empty() {
                video_links = "N/A".into();
            }

            let mut caption_links = String::new();

            if let Some(mut captions) = video.captions.clone() {
                captions.list.retain(|ref caption| caption.source.contains("en_US"));

                for cap in &captions.list {
                    let _ = write!(caption_links, "[{}]({}) ({})\n",
                        cap.label,
                        cap.source,
                        cap.locale);
                }

                if caption_links.is_empty() {
                    caption_links = "N/A".into();
                }
            } else {
                caption_links = "N/A".into();
            }

            let first_video = match video.videos.list.first() {
                Some(val) => val,
                None => return Err(CommandError::from(get_msg!("vlive/error/no_videos"))),
            };

            let duration = {
                let minutes = first_video.duration as u64 / 60;
                let seconds = first_video.duration as u64 % 60;

                format!("{}min {}sec", minutes, seconds)
            };

            if let Err(e) = msg.channel_id.send_message(|m| m
                .embed(|e| {
                    let mut e = e
                        .title(&video.meta.subject)
                        .url(&video.meta.url)
                        .image(&video.meta.cover.source)
                        .field("Duration", &duration, true)
                        .field("Video Links", &video_links, false)
                        .field("Caption Links", &caption_links, false);
                    
                    if is_ch_plus {
                        e = e.description("<:channel_plus:441720556212060160> **Requires CHANNEL+ subscription** (30 second preview)");
                    }

                    e
                })
            ) {
                warn!("Error sending vlive embed: {}", e);
            }
        },
        _ => {
            return Err(CommandError::from(get_msg!("vlive/error/missing_or_invalid_subcommand")));
        }
    }
});

command!(vlivenotif_add(ctx, msg, args) {
    let guild_id = match msg.guild_id {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("error/no_guild"))),
    };

    let query = args.rest();

    // regex for a discord channel id
    lazy_static! {
        static ref RE: Regex = Regex::new(r"(<#)?\d{17,18}>?").unwrap();
    }

    let start;
    let end;

    // search for discord channel id
    let discord_channel = match RE.find(query) {
        Some(mat) => {
            start = mat.start();
            end = mat.end();

            arg_types::ChannelArg::new()
                .string(mat.as_str())
                .guild(msg.guild())
                .error(get_msg!("vlive/error/invalid_channel"))
                .get()?
        },
        None => return Err(CommandError::from(get_msg!("vlive/error/invalid_channel"))),
    };

    // string slice before the discord channel, removing spaces in prefix/suffix
    let vlive_channel_query = &query[..start].trim_matches(' ');

    if vlive_channel_query.is_empty() {
        return Err(CommandError::from(get_msg!("vlive/error/missing_channel")));
    }

    // string slice after discord channel
    let role_name = &query[end..].trim_matches(' ');

    // only check for role if there is content after the channel
    let mention_role = if !role_name.is_empty() {
        Some(arg_types::RoleArg::new()
            .string(role_name)
            .guild(msg.guild())
            .error(get_msg!("vlive/error/invalid_role"))
            .get()?)
    } else {
        None
    };

    let _ = msg.channel_id.broadcast_typing();

    let client = Client::new();

    let channels = match client.get_channel_list() {
        Ok(val) => val,
        Err(why) => {
            warn_discord!("Err searching vlive '{}': {:?}", query, why);

            return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
        },
    };
    
    // search channel in list
    let channel = match channels.find_partial_channel_or_code(vlive_channel_query) {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("vlive/error/no_search_results"))),
    };

    // get channel code
    let channel_code = match channel.code {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("vlive/error/invalid_channel"))),
    };

    // fetch decoded channel seq
    let channel_seq = match client.decode_channel_code(channel_code) {
        Ok(val) => val,
        Err(e) => {
            warn_discord!("Error decoding channel: {}", e);

            return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
        }
    };

    // get channel videos
    let channel_data = match client.get_channel_video_list(channel_seq as u32, 10, 1) {
        Ok(val) => val,
        Err(e) => {
            warn_discord!("Error decoding channel: {}", e);

            return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
        }
    };

    let pool = get_pool(ctx);

    let guild_vlive_channels = match pool.get_guild_vlive_channels(guild_id.0) {
        Ok(val) => val,
        Err(e) => {
            warn_discord!("Error fetching guild vlive channels: {}", e);

            return Err(CommandError::from("error/unknown_error"))
        },
    };

    // check if already has channel
    if guild_vlive_channels
        .iter()
        .any(|x| x.channel_seq == channel_seq as i32 && x.discord_channel as u64 == discord_channel) {
        
        return Err(CommandError::from(get_msg!("vlive/error/already_added_channel")));
    }

    // add to db
    pool.add_vlive_channel(channel_seq as i32,
        &channel_data.channel_info.channel_code,
        &channel_data.channel_info.channel_name,
        guild_id.0,
        discord_channel,
        mention_role.clone().map(|x| x.id.0),
    );

    // add all current videos to db
    for video in channel_data.video_list {
        pool.add_vlive_video(channel_seq as i32, video.video_seq as i32);
    }

    if let Some(role) = mention_role {
        let _ = msg.channel_id.say(get_msg!("vlive/info/added_notification_with_mention",
            channel_data.channel_info.channel_name, discord_channel, role.name));
    } else {
        let _ = msg.channel_id.say(get_msg!("vlive/info/added_notification",
            channel_data.channel_info.channel_name, discord_channel));
    }
});


command!(vlivenotif_list(ctx, msg, _args) {
    let guild_id = match msg.guild_id {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("error/no_guild"))),
    };

    let pool = get_pool(ctx);

    let channels = match pool.get_guild_vlive_channels(guild_id.0) {
        Ok(val) => val,
        Err(e) => {
            warn_discord!("Error while listing vlive channels: {}", e);

            return Err(CommandError::from(get_msg!("vlive/error/failed_list")));
        }
    };

    if channels.is_empty() {
        let _ = msg.channel_id.say(get_msg!("vlive/info/no_notifications"));
        return Ok(());
    }

    let mut s = String::new();

    for channel in channels {
        if let Some(role) = channel.mention_role {
            let _ = writeln!(s, "<#{}> - [{}](http://channels.vlive.tv/{}) (mentions <@&{}>)",
                channel.discord_channel, channel.channel_name, channel.channel_code, role);
        } else {
            let _ = writeln!(s, "<#{}> - [{}](http://channels.vlive.tv/{})",
                channel.discord_channel, channel.channel_name, channel.channel_code);
        }
    }

    let _ = msg.channel_id.send_message(|m| m
        .embed(|e| e
            .author(|a| a
                .name("VLive notifications")
                .icon_url("https://i.imgur.com/NzGrmho.jpg")
            )
            .description(&s)
            .colour(0x54f7ff)
        )
    );
});

command!(vlivenotif_delete(ctx, msg, args) {
    let pool = get_pool(ctx);

    let query = args.rest();

    lazy_static! {
        static ref RE: Regex = Regex::new(r"(<#)?\d{17,18}>?").unwrap();
    }

    let start;

    // search for discord channel id
    let discord_channel = match RE.find(query) {
        Some(mat) => {
            start = mat.start();

            arg_types::ChannelArg::new()
                .string(mat.as_str())
                .guild(msg.guild())
                .error(get_msg!("vlive/error/invalid_channel"))
                .get()?
        },
        None => return Err(CommandError::from(get_msg!("vlive/error/invalid_channel"))),
    };

    let vlive_channel_query = &query[..start].trim_matches(' ');

    if vlive_channel_query.is_empty() {
        return Err(CommandError::from(get_msg!("vlive/error/missing_channel")));
    }

    let _ = msg.channel_id.broadcast_typing();

    let client = Client::new();

    let channels = match client.get_channel_list() {
        Ok(val) => val,
        Err(why) => {
            warn_discord!("Err searching vlive '{}': {:?}", vlive_channel_query, why);

            return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
        },
    };
    
    // search channel in list
    let channel = match channels.find_partial_channel_or_code(vlive_channel_query) {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("vlive/error/no_search_results"))),
    };

    // get channel code
    let channel_code = match channel.code {
        Some(val) => val,
        None => return Err(CommandError::from(get_msg!("vlive/error/invalid_channel"))),
    };

    // fetch decoded channel seq
    let channel_seq = match client.decode_channel_code(channel_code) {
        Ok(val) => val,
        Err(e) => {
            warn_discord!("Error decoding channel: {}", e);

            return Err(CommandError::from(get_msg!("vlive/error/failed_fetch_data")));
        }
    };

    match pool.delete_vlive_channel(channel_seq as i32, discord_channel) {
        Ok(count) => {
            if count == 0 {
                return Err(CommandError::from(get_msg!("vlive/error/delete_invalid")));
            }
            let _ = msg.channel_id.say(get_msg!("vlive/info/deleted_channel", channel.name));
        },
        Err(e) => {
            warn_discord!("Error deleting vlive channel: {}", e);

            let _ = msg.channel_id.say(get_msg!("vlive/error/failed_delete_channel"));
        },
    }
});
