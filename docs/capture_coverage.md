# Capture coverage

Capture: `requests_20261005_new.jsonl` — 51 request groups, 24 client commands, 118 server message types (215 packets).

## Client commands

| cmd | command | calls | handler | tier | response chain |
| --- | --- | ---: | --- | --- | --- |
| 10054 | CS_PUBLIC_CHAT_SETTING | 10 | yes | already handled | `10055` |
| 10057 | CS_REQ_MODULE_READ | 1 | yes | already handled | `10058` |
| 11000 | CS_ACCOUNT_LOGIN | 1 | yes | already handled | `10002`, `11001`, `12000`, `12001`, `12009` |
| 11005 | CS_ENTER_WORLD | 1 | yes | already handled | — |
| 12033 | CS_PLAYER_HOMEPAGE_INFO | 1 | yes | already handled | — |
| 13044 | CS_SET_READY | 1 | yes | already handled | — |
| 13046 | CS_CHANGE_HERO | 1 | yes | already handled | — |
| 13061 | CS_CANNOT_DEL_HERO_LIST | 1 | yes | already handled | — |
| 16005 | CS_MAIL_READ | 3 | yes | already handled | `16006` |
| 16007 | CS_MAIL_ENCLOSURE_REC | 1 | yes | already handled | `10059`, `16002`, `16008`, `17001` |
| 17009 | CS_SHOP_TYPE_DATA | 2 | yes | already handled | `17010` |
| 18006 | CS_HERO_BIOGRAPHY_INFO | 1 | yes | already handled | `10021`, `10032`, `10056`, `10059`, `10071`, `10101`, `12006`, `12009`, `12018`, `12020`, `12022`, `12034`, `12036`, `12051`, `12053`, `12055`, `12058`, `12062`, `12067`, `12100`, `12101`, `12102`, `12103`, `12104`, `12105`, `12106` ⚠, `12160`, `12210` ⚠, `12220` ⚠, `13041`, `13050`, `13060`, `13107`, `13142`, `13200`, `13220`, `13280`, `13350`, `13362`, `13370`, `15001`, `15003`, `15013`, `15028`, `16001`, `17000`, `17006`, `18000`, `18001`, `18054`, `18062`, `18120`, `19002`, `19050`, `19601`, `19910` ⚠, `20111`, `21001`, `23002`, `23008`, `24021`, `24033`, `24041`, `24066`, `24095`, `24097`, `24106`, `24112`, `24131`, `24201`, `24206`, `24212`, `24220`, `24240`, `24250`, `24271`, `24276`, `24351`, `24372`, `24402`, `24462`, `24490` |
| 20100 | CS_BATTLE_FIELD_ENTER | 1 | yes | already handled | `13045`, `13047`, `13062`, `20101` |
| 20102 | CS_BATTLE_START | 1 | yes | already handled | — |
| 20104 | CS_BATTLE_VIDEO_END | 14 | yes | already handled | `10032`, `10059`, `12002`, `12003`, `12010`, `12051`, `12105`, `13003`, `13007`, `13090`, `17001`, `18000`, `20103`, `20105`, `20106`, `20125`, `21073`, `24007`, `24024`, `24027`, `24117` |
| 20113 | CS_BATTLE_AUTO | 1 | yes | already handled | `10059`, `12100`, `20103`, `20114`, `20125` |
| 24022 | CS_GAIN_ACHIEVEMENT_AWARD | 1 | yes | already handled | `12003`, `24023`, `24024`, `24027` |
| 24065 | CS_GAIN_SEVEN_DAY_REWARD | 1 | yes | already handled | `17001`, `17013`, `24066` |
| 24096 | CS_DIRECT_GIFT_PANEL | 2 | yes | already handled | `24097` |
| 24098 | CS_DIRECT_GIFT_BUY | 1 | yes | already handled | `12003`, `24097`, `24099` |
| 24111 | CS_NOVICE_TRAINING_PANEL | 2 | yes | already handled | `12003`, `17001`, `17013`, `24112`, `24114` |
| 24113 | CS_NOVICE_TRAINING_RECEIVE_TASK | 1 | yes | already handled | — |
| 24221 | CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE | 1 | yes | already handled | `10059`, `17001`, `17013`, `24222` |
| 24270 | CS_GAIN_OPEN_SERVER_SIGN_REWARD | 1 | yes | already handled | `17001`, `17013`, `24271` |

## Server responses

| cmd | message | packets | replay encode | schema problems | data file |
| --- | --- | ---: | --- | --- | --- |
| 10055 | SC_PUBLIC_CHAT_SETTING | 10 | re-encodable | — | — |
| 10058 | SC_RES_MODULE_READ | 1 | re-encodable | — | — |
| 10002 | SC_SYS_DATE | 1 | re-encodable | — | — |
| 11001 | SC_ACCOUNT_LOGIN | 1 | re-encodable | — | — |
| 12000 | SC_PLAYER_END_DATA | 1 | re-encodable | — | — |
| 12001 | SC_PLAYER_BASE_DATA | 1 | re-encodable | — | `login/player_base_data.json` |
| 12009 | SC_FUNCTION_OPEN_LIST | 2 | re-encodable | — | `login/function_open_list.json` |
| 16006 | SC_MAIL_READ | 3 | re-encodable | — | — |
| 10059 | SC_NEW_UNREAD | 8 | re-encodable | — | — |
| 16002 | SC_MAIL_ADD | 1 | re-encodable | — | — |
| 16008 | SC_MAIL_ENCLOSURE_REC | 1 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 17010 | SC_SHOP_TYPE_DATA | 2 | re-encodable | — | `shop/shop_type_1.json`, `shop/shop_type_2.json`, `shop/shop_type_3.json`, `shop/shop_type_4.json`, `shop/shop_type_5.json`, `shop/shop_type_default.json` |
| 10021 | SC_PAY_DATA | 1 | re-encodable | — | `hero_biography/pay_data.json` |
| 10032 | SC_SYSTEM_ANNOUNCE | 2 | re-encodable | — | `hero_biography/system_annnounce.json` |
| 10056 | SC_RES_ALL_MODULE_READ | 1 | re-encodable | — | `hero_biography/res_all_module_read.json` |
| 10059 | SC_NEW_UNREAD | 8 | re-encodable | — | — |
| 10071 | SC_GET_SERVER_STATE | 1 | re-encodable | — | `hero_biography/server_state.json` |
| 10101 | SC_SETTING | 1 | re-encodable | — | `hero_biography/settings.json` |
| 12006 | SC_TODAY_NOT_NOTICE | 1 | re-encodable | — | `hero_biography/today_not_notice.json` |
| 12009 | SC_FUNCTION_OPEN_LIST | 2 | re-encodable | — | `login/function_open_list.json` |
| 12018 | SC_GET_AVATAR_LIST | 1 | re-encodable | — | `hero_biography/get_avatar_list.json` |
| 12020 | SC_GET_AVATAR_FRAME_LIST | 1 | re-encodable | — | `hero_biography/get_avatar_frame_list.json` |
| 12022 | SC_GET_DESIGNATION_LIST | 1 | re-encodable | — | `hero_biography/get_designation_list.json` |
| 12034 | SC_PLAYER_HOMEPAGE_INFO | 2 | re-encodable | — | `hero_biography/player_homepage.json`, `hero_biography/player_homepage_info.json` |
| 12036 | SC_GET_DIALOG_BOX_LIST | 1 | re-encodable | — | `hero_biography/get_dialog_box_list.json` |
| 12051 | SC_PLAYER_STAMINA_INFO | 4 | re-encodable | — | `hero_biography/player_stamina_info.json` |
| 12053 | SC_PLAYER_STORY_INFO | 1 | re-encodable | — | `hero_biography/player_story_info.json` |
| 12055 | SC_TITANIUM_EXCHANGE_GOLD_COIN_INFO | 1 | re-encodable | — | `hero_biography/titanium_exchange.json` |
| 12058 | SC_PLAYER_GUIDE_INFO | 1 | re-encodable | — | `hero_biography/player_guide_info.json` |
| 12062 | SC_IS_RENAME | 1 | re-encodable | — | `hero_biography/is_rename.json` |
| 12067 | SC_GET_BACKGROUND_LIST | 1 | re-encodable | — | `hero_biography/get_background_list.json` |
| 12100 | SC_MONSTER_MANUAL | 2 | re-encodable | — | `hero_biography/monster_manual.json` |
| 12101 | SC_EQUIP_SUIT_MANUAL | 1 | re-encodable | — | `hero_biography/equip_suit_manual.json` |
| 12102 | SC_BRACELET_MANUAL | 1 | re-encodable | — | `hero_biography/bracelet_manual.json` |
| 12103 | SC_MUSIC_MANUAL | 1 | re-encodable | — | `hero_biography/music_manual.json` |
| 12104 | SC_STORY_MANUAL | 1 | re-encodable | — | `hero_biography/story_manual.json` |
| 12105 | SC_WORLD_MANUAL | 2 | re-encodable | — | `hero_biography/world_manual.json` |
| 12106 | UNKNOWN(12106) | 1 | payload not decoded | — | — |
| 12160 | SC_DIALOGUE_PANEL | 1 | re-encodable | — | `hero_biography/dialogue_panel.json` |
| 12210 | UNKNOWN(12210) | 1 | payload not decoded | — | — |
| 12220 | UNKNOWN(12220) | 1 | payload not decoded | — | — |
| 13041 | SC_HERO_FORMATION | 1 | re-encodable | — | `hero_biography/hero_formation.json` |
| 13050 | SC_RECRUIT_INFO | 1 | re-encodable | — | `hero_biography/recruit_info.json` |
| 13060 | SC_HERO_PRE_LIST | 1 | re-encodable | — | `hero_biography/hero_pre_list.json` |
| 13107 | SC_FASHION_INFO | 1 | re-encodable | — | `hero_biography/fashion_info.json` |
| 13142 | SC_RELATION_REWARD | 1 | re-encodable | — | `hero_biography/relation_reward.json` |
| 13200 | SC_HERO_LV_REWARD | 1 | re-encodable | — | `hero_biography/hero_lv_reward.json` |
| 13220 | SC_HERO_ASSIST_FIGHT_SKILL | 1 | re-encodable | — | `hero_biography/assist_fight_skill.json` |
| 13280 | SC_HERO_ACTION_LIST | 1 | re-encodable | — | `hero_biography/hero_action_list.json` |
| 13350 | SC_HERO_FASHION_HAVE_INFO | 1 | re-encodable | — | `hero_biography/hero_fashion_have_info.json` |
| 13362 | SC_ACT_FETTER_INFO | 1 | re-encodable | — | `hero_biography/act_fetter_info.json` |
| 13370 | SC_FASHION_SCENE_PANEL | 1 | re-encodable | — | — |
| 15001 | SC_FRIEND_LIST | 1 | re-encodable | — | `hero_biography/friend_list.json` |
| 15003 | SC_FRIEND_APPLY_LIST | 1 | re-encodable | — | `hero_biography/friend_apply_list.json` |
| 15013 | SC_BLACK_LIST | 1 | re-encodable | — | `hero_biography/black_list.json` |
| 15028 | SC_FRIEND_GIFT_PANEL | 1 | re-encodable | — | `hero_biography/friend_gift_panel.json` |
| 16001 | SC_MAIL_LIST | 1 | re-encodable | — | `hero_biography/mail_list.json` |
| 17000 | SC_BAG_INIT | 8 | re-encodable | — | `hero_biography/bag_init_type1.json`, `hero_biography/bag_init_type2.json`, `hero_biography/bag_init_type3.json`, `hero_biography/bag_init_type4.json`, `hero_biography/bag_init_type5.json`, `hero_biography/bag_init_type6.json`, `hero_biography/bag_init_type7.json`, `hero_biography/bag_init_type8.json` |
| 17006 | SC_SHOP_DATA | 1 | re-encodable | — | `hero_biography/shop_data.json` |
| 18000 | SC_MAIN_STORY_INFO | 2 | re-encodable | — | `hero_biography/main_story_info.json` |
| 18001 | SC_DUP_DATA | 1 | re-encodable | — | `hero_biography/dup_data.json` |
| 18054 | SC_MAIN_STORY_STAGE_AWARD_LIST | 1 | re-encodable | — | `hero_biography/main_story_award_list.json` |
| 18062 | SC_CHIP_DUP_PICK_SUIT | 1 | re-encodable | — | `hero_biography/chip_dup_pick_suit.json` |
| 18120 | SC_PARKOUR_PANEL | 1 | re-encodable | — | `hero_biography/parked_panel.json` |
| 19002 | SC_COLLEGE_DELEGATION_HALL_INFO | 1 | re-encodable | — | `hero_biography/college_delegation.json` |
| 19050 | SC_ACTIVITY_OPEN_INFO | 2 | re-encodable | — | `hero_biography/open_activity_open_info.json` |
| 19601 | SC_HERO_TRY_INFO | 1 | re-encodable | — | `hero_biography/hero_try_info.json` |
| 19910 | UNKNOWN(19910) | 1 | payload not decoded | — | — |
| 20111 | SC_BATTLE_REPLAY_INFOS | 1 | re-encodable | — | `hero_biography/battle_replay_info.json` |
| 21001 | SC_FORCES_PANEL | 1 | re-encodable | — | `hero_biography/force_panel.json` |
| 23002 | SC_GUILD_PANEL | 1 | re-encodable | — | `hero_biography/guild_panel.json` |
| 23008 | SC_REFRESH_RECOMMEND_GUILDS | 1 | re-encodable | — | `hero_biography/refresh_recommend_guilds.json` |
| 24021 | SC_ACHIEVEMENT_PANEL_INFO | 1 | re-encodable | — | `hero_biography/achievement_panel.json` |
| 24033 | SC_SIGN_PANEL | 1 | re-encodable | — | `hero_biography/sign_panel.json` |
| 24041 | SC_NOVICE_TARGET_PANEL_INFO | 1 | re-encodable | — | `hero_biography/novice_target_panel.json` |
| 24066 | SC_SEVEN_DAY_PANEL_INFO | 2 | re-encodable | — | `hero_biography/seven_day_panel.json` |
| 24095 | SC_MONTH_CARD_PANEL | 1 | re-encodable | — | `hero_biography/month_card_panel.json` |
| 24097 | SC_DIRECT_GIFT_PANEL | 4 | re-encodable | — | `hero_biography/direct_gift_panel.json`, `shop/direct_gift_panel.json` |
| 24106 | SC_ACC_PAY_PANEL | 1 | re-encodable | — | `hero_biography/acc_pay_panel.json` |
| 24112 | SC_NOVICE_TRAINING_PANEL | 3 | re-encodable | — | `hero_biography/novice_training_panel.json` |
| 24131 | SC_FASHION_SHOP_PANEL | 1 | re-encodable | — | `hero_biography/fashion_shop_panel.json` |
| 24201 | SC_LEVEL_GIFT_PANEL | 1 | re-encodable | — | `hero_biography/level_gift_panel.json` |
| 24206 | SC_FUND_PANEL | 1 | re-encodable | — | `hero_biography/fund_panel.json` |
| 24212 | SC_FIRST_PAY_PANEL | 1 | re-encodable | — | `hero_biography/first_pay_panel.json` |
| 24220 | SC_ACTIVITY_NOVICE_RECRUIT_HERO_PANEL | 1 | re-encodable | — | `hero_biography/activity_novice_recruit.json` |
| 24240 | SC_ACTIVITY_NOVICE_UPGRADE_PANEL | 1 | re-encodable | — | `hero_biography/activity_novice_upgrade.json` |
| 24250 | SC_ACTIVITY_NOVICE_START | 1 | re-encodable | — | `hero_biography/activity_novice_start.json` |
| 24271 | SC_OPEN_SERVER_SIGN_PANEL_INFO | 2 | re-encodable | — | `hero_biography/open_server_sign_panel.json` |
| 24276 | SC_PLATFORM_ACHIEVE_PANEL | 1 | re-encodable | — | `hero_biography/platform_achieve_panel.json` |
| 24351 | SC_STAMINA_MONTH_CARD_PANEL | 1 | re-encodable | — | `hero_biography/stamina_month_card_panel.json` |
| 24372 | SC_DOWN_GIFT_SHOW | 1 | re-encodable | — | `hero_biography/down_gift_show.json` |
| 24402 | SC_LIMITED_GIFT_PANEL | 1 | re-encodable | — | `hero_biography/limited_gift_panel.json` |
| 24462 | SC_ACTIVITY_DAY_REWARD_PANEL | 1 | re-encodable | — | `hero_biography/activity_day_reward_panel.json` |
| 24490 | SC_ACTIVITY_EXPIRED_GOODS | 1 | re-encodable | — | — |
| 13045 | SC_SET_READY | 1 | re-encodable | — | — |
| 13047 | SC_CHANGE_HERO | 1 | re-encodable | — | — |
| 13062 | SC_CANNOT_DEL_HERO_LIST | 1 | re-encodable | — | — |
| 20101 | SC_BATTLE_FIELD_INFO | 1 | re-encodable | — | — |
| 10032 | SC_SYSTEM_ANNOUNCE | 2 | re-encodable | — | `hero_biography/system_annnounce.json` |
| 10059 | SC_NEW_UNREAD | 8 | re-encodable | — | — |
| 12002 | SC_PLAYER_UPDATE_ATTR_INT | 1 | re-encodable | — | — |
| 12003 | SC_PLAYER_UPDATE_ATTR_BIGINT | 7 | re-encodable | — | — |
| 12010 | SC_ADD_FUNCTION_OPEN | 1 | re-encodable | — | — |
| 12051 | SC_PLAYER_STAMINA_INFO | 4 | re-encodable | — | `hero_biography/player_stamina_info.json` |
| 12105 | SC_WORLD_MANUAL | 2 | re-encodable | — | `hero_biography/world_manual.json` |
| 13003 | SC_HERO_UPDATE_ATTR | 1 | re-encodable | — | — |
| 13007 | SC_HERO_LEVELUP | 2 | re-encodable | — | — |
| 13090 | SC_HERO_UPDATE_ATTR_BIGINT | 5 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 18000 | SC_MAIN_STORY_INFO | 2 | re-encodable | — | `hero_biography/main_story_info.json` |
| 20103 | SC_BATTLE_ACTION | 14 | re-encodable | — | — |
| 20105 | SC_BATTLE_ACTION_END | 6 | re-encodable | — | — |
| 20106 | SC_BATTLE_RESULT | 1 | re-encodable | — | — |
| 20125 | SC_BATTLE_ACTION_NOTICE | 2 | re-encodable | — | — |
| 21073 | SC_UPDATE_FORCES_TASK_INFO | 2 | re-encodable | — | — |
| 24007 | SC_UPDATE_TASK_INFO | 5 | re-encodable | — | — |
| 24024 | SC_UPDATE_ACHIEVEMENT_INFO | 10 | re-encodable | — | — |
| 24027 | SC_UPDATE_COMPLETE_ACHIEVE_INFO | 3 | re-encodable | — | — |
| 24117 | SC_UPDATE_NOVICE_TRAINING_TASK | 1 | re-encodable | — | — |
| 10059 | SC_NEW_UNREAD | 8 | re-encodable | — | — |
| 12100 | SC_MONSTER_MANUAL | 2 | re-encodable | — | `hero_biography/monster_manual.json` |
| 20103 | SC_BATTLE_ACTION | 14 | re-encodable | — | — |
| 20114 | SC_BATTLE_AUTO | 1 | re-encodable | — | — |
| 20125 | SC_BATTLE_ACTION_NOTICE | 2 | re-encodable | — | — |
| 12003 | SC_PLAYER_UPDATE_ATTR_BIGINT | 7 | re-encodable | — | — |
| 24023 | SC_GAIN_ACHIEVEMENT_AWARD | 1 | re-encodable | — | — |
| 24024 | SC_UPDATE_ACHIEVEMENT_INFO | 10 | re-encodable | — | — |
| 24027 | SC_UPDATE_COMPLETE_ACHIEVE_INFO | 3 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 17013 | SC_PROP_AWARD_SEND | 4 | re-encodable | — | — |
| 24066 | SC_SEVEN_DAY_PANEL_INFO | 2 | re-encodable | — | `hero_biography/seven_day_panel.json` |
| 24097 | SC_DIRECT_GIFT_PANEL | 4 | re-encodable | — | `hero_biography/direct_gift_panel.json`, `shop/direct_gift_panel.json` |
| 12003 | SC_PLAYER_UPDATE_ATTR_BIGINT | 7 | re-encodable | — | — |
| 24097 | SC_DIRECT_GIFT_PANEL | 4 | re-encodable | — | `hero_biography/direct_gift_panel.json`, `shop/direct_gift_panel.json` |
| 24099 | SC_DIRECT_GIFT_BUY | 1 | re-encodable | — | — |
| 12003 | SC_PLAYER_UPDATE_ATTR_BIGINT | 7 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 17013 | SC_PROP_AWARD_SEND | 4 | re-encodable | — | — |
| 24112 | SC_NOVICE_TRAINING_PANEL | 3 | re-encodable | — | `hero_biography/novice_training_panel.json` |
| 24114 | SC_NOVICE_TRAINING_RECEIVE_TASK | 1 | re-encodable | — | — |
| 10059 | SC_NEW_UNREAD | 8 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 17013 | SC_PROP_AWARD_SEND | 4 | re-encodable | — | — |
| 24222 | SC_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE | 1 | re-encodable | — | — |
| 17001 | SC_BAG_UPDATE | 6 | re-encodable | — | — |
| 17013 | SC_PROP_AWARD_SEND | 4 | re-encodable | — | — |
| 24271 | SC_OPEN_SERVER_SIGN_PANEL_INFO | 2 | re-encodable | — | `hero_biography/open_server_sign_panel.json` |

## Data-loader coverage

86 of 96 JSON data files for commands present in this capture can be produced from it (105 files are declared by `data_loader.rs` in total).

Not present in the capture:

- `hero_biography/activity_novice_turntable.json` (SC_ACTIVITY_NOVICE_TURNTABLE_PANEL, cmd 24300)
- `hero_biography/activity_pay_sign2_panel.json` (SC_ACTIVITY_PAY_SIGN2_PANEL, cmd 24425)
- `hero_biography/pack_bag_panel.json` (SC_PACK_BAG_PANEL, cmd 18181)
- `hero_biography/happy_farm_field_list.json` (SC_HAPPY_FARM_FIELD_LIST, cmd 18190)
- `hero_biography/happy_farm_order_list.json` (SC_HAPPY_FARM_ORDER_LIST, cmd 18191)
- `shop/shop_type_2.json` (SC_SHOP_TYPE_DATA, cmd 17010)
- `shop/shop_type_3.json` (SC_SHOP_TYPE_DATA, cmd 17010)
- `shop/shop_type_4.json` (SC_SHOP_TYPE_DATA, cmd 17010)
- `shop/shop_type_5.json` (SC_SHOP_TYPE_DATA, cmd 17010)
